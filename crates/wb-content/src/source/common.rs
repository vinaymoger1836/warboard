//! Types shared by several content kinds: design notes, bands, classes.

/// Derives for every source type: serde, plus JSON Schema and TypeScript
/// export behind the `schema` and `ts` features.
macro_rules! source_type {
    ($($item:item)*) => {
        $(
            #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
            #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
            #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
            $item
        )*
    };
}

/// Like `source_type!` for field-less enums that are also map keys.
macro_rules! source_enum {
    ($($item:item)*) => {
        $(
            #[derive(
                Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
                serde::Serialize, serde::Deserialize,
            )]
            #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
            #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
            $item
        )*
    };
}

pub(crate) use {source_enum, source_type};

source_type! {
    /// What an entry is meant to be in the game (doc 04 §4, ADR-036). Required
    /// on every entry except archetypes, worlds and realism profiles.
    #[serde(deny_unknown_fields)]
    pub struct Design {
        /// Class archetype id (`content/archetypes/`). Required on catalogue
        /// entries (platforms, systems, sensors, weapons, EW suites,
        /// countermeasures, data links); key stats should sit in its bands.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub archetype: Option<String>,
        /// One or two lines on the entry's role and balance.
        pub note: String,
    }
}

source_enum! {
    /// Radio frequency band (IEEE letter designations).
    pub enum Band {
        HF,
        VHF,
        UHF,
        L,
        S,
        C,
        X,
        Ku,
        K,
        Ka,
    }

    /// Physical domain a platform or contact moves in.
    #[serde(rename_all = "snake_case")]
    pub enum Domain {
        Air,
        Surface,
        Subsurface,
        Ground,
        Space,
    }

    /// What a target is, for weapon `pk_base` tables and contact classes.
    #[serde(rename_all = "snake_case")]
    pub enum TargetClass {
        Fighter,
        Bomber,
        Attack,
        Transport,
        Tanker,
        Aew,
        EwAircraft,
        MaritimePatrol,
        Helicopter,
        Uav,
        CruiseMissile,
        AntiShipMissile,
        BallisticMissile,
        AntiRadiationMissile,
        AirToAirMissile,
        SurfaceToAirMissile,
        GuidedBomb,
        SurfaceShip,
        Submarine,
        GroundVehicle,
        RadarSite,
        Launcher,
        C2Node,
        Structure,
    }

    /// Weapon terminal guidance / seeker type.
    #[serde(rename_all = "snake_case")]
    pub enum Seeker {
        ActiveRadar,
        SemiActiveRadar,
        Ir,
        ImagingIr,
        AntiRadiation,
        SatnavIns,
        Laser,
        Command,
        None,
    }

    /// Kind of countermeasure.
    #[serde(rename_all = "snake_case")]
    pub enum CountermeasureType {
        Chaff,
        Flare,
        TowedDecoy,
        Dircm,
    }

    /// Air-to-air refuelling system.
    #[serde(rename_all = "snake_case")]
    pub enum AarSystem {
        None,
        Boom,
        Probe,
    }

    /// Emission control state.
    #[serde(rename_all = "snake_case")]
    pub enum Emcon {
        On,
        Off,
        Blink,
    }
}

/// Serde default helper.
pub(crate) fn one() -> u32 {
    1
}

/// Serde default helper.
pub(crate) fn no_aar() -> AarSystem {
    AarSystem::None
}
