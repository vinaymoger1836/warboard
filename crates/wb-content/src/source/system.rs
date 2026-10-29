//! `system` (composite systems deployed as a site or group) and `loadout`.

use super::common::{Emcon, Provenance, source_enum, source_type};
use std::collections::BTreeMap;

source_enum! {
    #[serde(rename_all = "snake_case")]
    pub enum DeployAs {
        /// A fixed site: components placed around one point.
        Site,
        /// A mobile group moving together.
        Group,
    }

    #[serde(rename_all = "snake_case")]
    pub enum ComponentRole {
        Surveillance,
        FireControl,
        C2,
        Launcher,
        Support,
    }
}

source_type! {
    /// A composite system such as an S-400 battalion (doc 04 §3.4). Each
    /// component is its own damageable entity on the map.
    #[serde(deny_unknown_fields)]
    pub struct System {
        pub id: String,
        pub name: String,
        /// Short form for map labels (`S-400`); the name when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub short_name: Option<String>,
        pub deploy_as: DeployAs,
        /// Time from arrival to ready to fire.
        pub setup_min: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub teardown_min: Option<f64>,
        pub components: Vec<SystemComponent>,
        pub emcon_default: Emcon,
        /// Can join an IADS network and share tracks.
        #[serde(default)]
        pub iads_capable: bool,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct SystemComponent {
        pub platform: String,
        pub count: u32,
        pub role: ComponentRole,
        /// Weapon id → rounds per component unit (e.g. per launcher).
        #[serde(default)]
        pub magazine: BTreeMap<String, u32>,
    }

    /// A named loadout template for one platform.
    #[serde(deny_unknown_fields)]
    pub struct Loadout {
        pub id: String,
        pub name: String,
        pub platform: String,
        /// Free-text mission tags, e.g. `air_superiority`, `sead`.
        #[serde(default)]
        pub roles: Vec<String>,
        pub items: Vec<LoadoutItem>,
        pub provenance: Provenance,
    }

    #[serde(deny_unknown_fields)]
    pub struct LoadoutItem {
        /// Station id on the platform.
        pub station: String,
        pub weapon: String,
        pub count: u32,
    }
}
