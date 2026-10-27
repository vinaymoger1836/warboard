//! Types shared by several content kinds: provenance, bands, classes.

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

source_enum! {
    /// How far an entry's values can be trusted (doc 04 §4).
    #[serde(rename_all = "snake_case")]
    pub enum Confidence {
        /// Official figures.
        High,
        /// Consistent open-source reporting.
        Medium,
        /// A single or disputed source.
        Low,
        /// A reasoned guess.
        Estimate,
    }
}

source_type! {
    /// Where the values of an entry come from. Required on every entry.
    #[serde(deny_unknown_fields)]
    pub struct Provenance {
        pub confidence: Confidence,
        /// Field paths (e.g. `signature.rcs_m2`, `stations[0].max_kg`) that are
        /// guesses even if the entry is otherwise solid.
        #[serde(default)]
        pub estimates: Vec<String>,
        /// Public sources. Empty only for custom entries.
        #[serde(default)]
        pub sources: Vec<Source>,
        /// Free text: disagreements between sources, ranges, reasoning.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub notes: Option<String>,
    }

    /// One public source.
    #[serde(deny_unknown_fields)]
    pub struct Source {
        pub title: String,
        pub url: String,
        /// Access date, `YYYY-MM-DD`.
        pub accessed: String,
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
        GpsIns,
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
