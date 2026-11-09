// One entry: a form generated from the JSON Schema (@rjsf/core) with the custom widgets,
// the YAML text, the archetype band bars and this file's problems. Edits go to the YAML
// in place (comments kept) and are checked live by wb-content in WebAssembly.
import Form from '@rjsf/core';
import { type RegistryFieldsType, type RegistryWidgetsType, getDefaultFormState } from '@rjsf/utils';
import validator from '@rjsf/validator-ajv8';
import { useMemo, useRef, useState } from 'react';
import { schemaFor, uiFor } from '../schema';
import { editFile, entry, fileOfId, get, kindOfFile, set, useStore } from '../store';
import { applyDelta, toData } from '../yamlEdit';
import { ArchetypeWidget, BandBars, MagazineField, RcsField, StationsField } from './widgets';

const fields: RegistryFieldsType = { rcs: RcsField, stations: StationsField, magazine: MagazineField };
const widgets: RegistryWidgetsType = { archetype: ArchetypeWidget };

/** Kinds a custom variant can be made of (doc 04 §3.6). */
const CATALOGUE = ['platform', 'system', 'sensor', 'weapon', 'ew_suite', 'countermeasure', 'datalink'];

export function EntryEditor({ id }: { id: string }) {
  const file = useStore((s) => fileOfId(id, s));
  const text = useStore((s) => (file ? s.files[file] : undefined));
  const info = useStore((s) => entry(id, s));
  const report = useStore((s) => s.report);
  const [tab, setTab] = useState<'form' | 'yaml'>('form');
  const data = useMemo(() => (text ? toData(text) : {}), [text]);
  const kind = info?.kind ?? (text ? kindOfFile(text) : null) ?? '';
  const schema = schemaFor(kind);
  // The form's own state (data plus the schema's defaults), to write only real changes.
  const formState = useMemo(
    () => (schema ? (getDefaultFormState(validator, schema, data, schema) as Record<string, unknown>) : data),
    [schema, data],
  );
  const prev = useRef(formState);
  prev.current = formState;
  if (!file || text === undefined) return <p className="hint">No file for {id}.</p>;
  const problems = [...(report?.errors ?? []), ...(report?.warnings ?? [])].filter((p) => p.file === `content/${file}`);
  const archetype = info?.archetype ?? ((data['design'] as { archetype?: string } | undefined)?.archetype ?? null);
  return (
    <div className="editor">
      <header className="card-head">
        <div>
          <h2>{typeof data['name'] === 'string' ? data['name'] : id}</h2>
          <div className="sub">
            <span className="mono">{kind}</span> · <span className="mono">{id}</span> · content/{file}
            {info?.extends && (
              <>
                {' '}
                · extends{' '}
                <button className="link" onClick={() => set({ selected: info.extends })}>
                  {info.extends}
                </button>
              </>
            )}
          </div>
        </div>
        <div className="actions">
          {CATALOGUE.includes(kind) && <CustomVariant id={id} file={file} kind={kind} />}
          <button className="cmd small" onClick={() => set({ view: 'compare', compare: [id, get().compare[1]] })}>
            COMPARE
          </button>
        </div>
      </header>
      {problems.length > 0 && (
        <ul className="problems">
          {problems.map((p, i) => (
            <li key={i} className={report?.errors.includes(p) ? 'bad' : 'warn'}>
              <span className="mono">{p.path || '(file)'}</span> {p.message}
            </li>
          ))}
        </ul>
      )}
      <div className="editor-body">
        <div className="editor-main">
          <div className="tabs">
            <button className={tab === 'form' ? 'tab on' : 'tab'} onClick={() => setTab('form')}>
              Form
            </button>
            <button className={tab === 'yaml' ? 'tab on' : 'tab'} onClick={() => setTab('yaml')}>
              YAML
            </button>
          </div>
          {tab === 'yaml' || !schema ? (
            <textarea className="yaml" spellCheck={false} value={text} onChange={(e) => editFile(file, e.target.value)} />
          ) : (
            <Form
              schema={schema}
              uiSchema={uiFor(kind)}
              formData={data}
              formContext={data}
              validator={validator}
              fields={fields}
              widgets={widgets}
              noValidate
              liveOmit={false}
              onChange={(e) => {
                const next = e.formData as Record<string, unknown> | undefined;
                if (!next) return;
                const updated = applyDelta(text, prev.current, next);
                prev.current = next;
                if (updated !== text) editFile(file, updated);
              }}
            />
          )}
        </div>
        <aside className="editor-side">
          <h3>Archetype bands</h3>
          <BandBars bands={info?.bands ?? []} archetype={archetype} />
          {info?.extends && <ResolvedNote id={id} />}
        </aside>
      </div>
    </div>
  );
}

function ResolvedNote({ id }: { id: string }) {
  const info = useStore((s) => entry(id, s));
  if (!info) return null;
  return <p className="hint">Bands show the resolved values: the parent's fields with this entry's overrides applied.</p>;
}

/** Makes `custom-<id>-…` that extends this entry, in the same folder (doc 04 §3.6). */
function CustomVariant({ id, file, kind }: { id: string; file: string; kind: string }) {
  const [open, setOpen] = useState(false);
  const [newId, setNewId] = useState(`custom-${id}-x`);
  const [name, setName] = useState('');
  const [err, setErr] = useState('');
  if (!open) {
    return (
      <button className="cmd small" onClick={() => setOpen(true)}>
        NEW CUSTOM VARIANT
      </button>
    );
  }
  const create = () => {
    const s = get();
    if (!/^[a-z0-9][a-z0-9-]*$/.test(newId)) return setErr('Id: lower-case letters, digits and hyphens');
    if (fileOfId(newId, s)) return setErr(`${newId} already exists`);
    const parent = entry(id, s);
    const pv = parent?.value ?? {};
    const design = (pv['design'] as { archetype?: string } | undefined) ?? {};
    const folder = file.split('/').slice(0, -1).join('/');
    const label = name.trim() || `${String(pv['name'] ?? id)} (custom)`;
    const text = [
      `id: ${newId}`,
      `kind: ${kind}`,
      'custom: true',
      `extends: ${id}`,
      `name: ${JSON.stringify(label)}`,
      'overrides: {}',
      `design: { archetype: ${design.archetype ?? 'TODO'}, note: ${JSON.stringify(`Custom variant of ${id}; say what changed and why`)} }`,
      '',
    ].join('\n');
    editFile(`${folder}/${newId}.yaml`, text);
    set({ selected: newId });
    setOpen(false);
  };
  return (
    <div className="inline-form">
      <input value={newId} onChange={(e) => setNewId(e.target.value)} aria-label="New id" />
      <input value={name} placeholder="Name" onChange={(e) => setName(e.target.value)} aria-label="Name" />
      <button className="cmd small" onClick={create}>
        CREATE
      </button>
      <button className="link" onClick={() => setOpen(false)}>
        Cancel
      </button>
      {err && <span className="bad">{err}</span>}
    </div>
  );
}
