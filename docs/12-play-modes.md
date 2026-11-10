# 12 — Play modes: controllers, side switching, hot-seat

**Decided 9 Nov 2026 (ADR-044). Built in P9.**

Each side of a war has a **controller**, Human or AI. One setting gives every mode:

| Mode | Side A | Side B | For |
|---|---|---|---|
| **vs AI** | Human | AI | Normal play |
| **Command both** | Human | Human (same person) | Solo sandbox: deploy and decide for both sides |
| **Hot-seat** | Human | Human (second player) | Two players taking turns on one device |
| **Watch** | AI | AI | Spectating; also the P9 AI-vs-AI soak test |

Online PvP stays later (doc 06 §13). Hot-seat needs no network: one device, one engine.

## 1. Engine

| Change | Rule |
|---|---|
| Setup | `controllers: { A: human \| ai, B: human \| ai }`, `ai_difficulty` per AI side, `mode` (`vs_ai`, `command_both`, `hot_seat`, `watch`). Part of the snapshot, like the rest of setup |
| AI | `wb-ai` runs only for sides whose controller is AI; it still reads only that side's picture |
| Orders | Every order carries `side`. Ids resolve within that side: contact ids are a per-side namespace (ADR-026), so contact 5 of side A is not contact 5 of side B. The engine rejects an order that names units, contacts or bases of another side: "Order rejected: unit 1203 belongs to the other side" |
| Views, details, events | Unchanged in content: one `SideView`, one details API and one event stream per side, as today. The worker builds the view of the side the client asks for (`set_view_side`); `details` and `contact_details` take the side |
| Approval requests | Belong to their side and wait in that side's queue; expiry rules as in doc 03 §7 |
| Saves | Setup (in the snapshot) records `controllers` and `mode`. A new **session** section records `active_side`, hot-seat settings, PIN hashes and `umpire_used`; it is **not part of the state hash**, so switching sides or handing over never changes the war. A hot-seat save opens at the handover screen of the active side |
| Umpire view | `umpire_view()` returns both sides exactly **only when `mode = command_both`** and the player turned it on. Refused by the API in every other mode. `wb-ai` cannot reach it (crate visibility). A war that ever used it is marked `umpire_used` in the save and the after-action report |

Determinism and replays are unchanged: orders are logged with their side and tick as before. Handover, turn timers and urgent-event interrupts are client behaviour, like sim speed (doc 02 §3): they only pause the clock.

## 2. Client

| Element | Behaviour |
|---|---|
| **Side toggle** | Top bar: active side's flag and nation id; click or **Tab** to switch. In Command both a third position, **Umpire**, appears when enabled (doc 10 §8) |
| **Colours** | The active side is always drawn as own (purple), the other side as enemy (blue). Contacts keep their estimate styling (dashed, "(our estimate)") |
| **Per-side UI state** | Each human side keeps its own camera, selection, open panels, alert feed, approval queue, operation planner state and event log filter; switching restores them |
| **Event streams** | The client keeps each human side's events apart and renders only the active side's. In hot-seat the waiting side's alerts, toasts and sounds are held until that side is active |
| **Orders** | Only the active side's units can be selected and ordered; the client sends the active side with every order |
| **Umpire view** | Both sides exact: side A in `side.own`, side B in `side.enemy`, no ellipses; read-only (no orders, no selection beyond the info card); banner "Umpire view: both sides visible" |
| **After-action report** | During the war, per side as today (the active side's); after the war, truth review for both, with the `umpire_used` mark if set |

### 2.1 Switching rules by mode

| Mode | Switch | Clock |
|---|---|---|
| vs AI | No toggle | Unchanged |
| Command both | Instant (toggle or Tab), no handover screen | Keeps running |
| Hot-seat | Through the handover screen (§3) | Pauses at handover; resumes when the next player continues |
| Watch | Toggle between the two sides' pictures and Umpire | Keeps running |

## 3. Hot-seat

**Handover screen:** full screen, no map, no panels, no alerts, no toasts: "Pass the device to the Vessmar player", side flag, and **Continue** (plus PIN entry if set). The clock is paused while it shows.

| Setting | Options | Default |
|---|---|---|
| Hand over | **On request**: the active player presses *Hand over*. **Turns**: also every 5, 15 or 30 sim minutes | On request |
| Urgent events for the waiting side | **Interrupt**: a launch against the waiting side's forces or a new approval request for it pauses and opens the handover screen, without saying why. **Don't interrupt**: the waiting side's units act on their ROE (self-defence still works) and requests wait or expire | Interrupt |
| PIN per side | Optional 4-digit PIN asked on Continue; stored as a salted hash in the save's session section. A courtesy between players, not security | Off |

Auto-slow (doc 03 §9) applies to the active side's events only; urgent events for the waiting side follow the setting above.

## 4. War setup screen

Adds a **Players** step after coalitions: for each coalition, Human or AI (with difficulty). Two humans ask "Same person (Command both) or two players (Hot-seat)?", and hot-seat shows the settings of §3. Two AIs give Watch. Quick starts default to Human vs AI.

## 5. Acceptance (P9)

- An order from side A naming a side B unit, contact or base is rejected with a reason, in every mode.
- Truth canary passes for **both** sides' views, details and events in Command both and Hot-seat.
- `umpire_view()` is refused outside Command both and when not enabled; a save that used it carries `umpire_used`.
- The handover screen renders no map, panels, alerts or toasts (browser test), and the clock does not advance while it shows.
- Switching sides restores each side's camera, selection and open panels.
- A hot-seat save reopens at the active side's handover screen; with a PIN, Continue needs it.
- AI runs only for AI-controlled sides (the event log shows no AI decisions for human sides).
- **G16:** the same scripted two-sided order log gives the same state hash whichever side's view (or the umpire view) was requested and however often the side was switched.
- Watch mode runs the 24 h soak.

## 6. As built (P9)

- **Engine:** `SideInfo` has `controller` and `difficulty`, `Sides` the `mode` (state hash format 6). The session (`wb_sim::session`) lives beside the world in `Sim` and in its own save section (save version 4). `Sim::umpire_view` checks the mode and the switch and marks `umpire_used`; the after-action report carries it. Refused orders read like unknown ids (ADR-046).
- **WebAssembly API:** `set_active_side`, `set_view_side` (255 = umpire), `set_umpire`, `set_hot_seat`, `set_pin`, `check_pin`, `session`; `details`, `contact_details`, `approvals`, `operations`, `report`, `inventory`, `ground_track`, `deployment` and `take_events` take the side (a human side, or either in Watch); `order` always uses the active side and refuses in the umpire view. `create_from_setup`, `nation_options` and `preview_setup` build wars from the setup screens.
- **Client:** menu and war setup (`apps/game/src/setup`), side toggle and Tab (`panels/SideToggle.tsx`), per-side UI slices (`store.ts`), play-mode control (`play.ts`), handover screen (`Handover.tsx`), deploy panel, umpire banner. A switch re-tints the territory in place, so the map and its camera stay.
- **Tests:** `crates/wb-sim/tests/play_modes.rs` (cross-side rejection in every mode, truth canary for both sides in Command both and Hot-seat, umpire rules, session not hashed, G16), `crates/wb-wasm/node/play-modes-check.mjs` (the browser API, run by `scripts/determinism-check.sh`), `apps/game/e2e` (Playwright: handover screen, PIN, hot-seat save, Command both switching, umpire view).
