use std::collections::BTreeMap;

use super::*;
use crate::backend_cli::{
    InstallStateWire, RuntimeCliClient, RuntimeResourceDetailsWire, RuntimeResourceRefWire,
};

/// Models the default workflow requires for Balanced analysis: Leap vocal
/// separation, Qwen transcription and alignment, RMVPE pitch, GAME medium
/// notes, and Basic Pitch onset evidence.
const STANDARD_MODELS: &[&str] = &[
    "bs_roformer_leap_xe90_vocals",
    "qwen3_asr_1_7b",
    "qwen3_forced_aligner_0_6b",
    "rmvpe",
    "game_1_0_3_medium",
    "basic_pitch",
];

/// Maximum analysis adds the JBM555, STARS and ROSVOT note experts it
/// requires and the FCPE and FireRed challengers it runs when present.
const MAXIMUM_MODELS: &[&str] = &[
    "bs_roformer_leap_xe90_vocals",
    "qwen3_asr_1_7b",
    "qwen3_forced_aligner_0_6b",
    "rmvpe",
    "game_1_0_3_medium",
    "basic_pitch",
    "jbm555_cectc_80",
    "stars",
    "rosvot",
    "fcpe",
    "firered_asr2_aed",
];

/// Every catalog model, including alternative separation strategies,
/// preprocessing, and the other GAME sizes.
const COMPLETE_MODELS: &[&str] = &[
    "bs_roformer_leap_xe90_vocals",
    "qwen3_asr_1_7b",
    "qwen3_forced_aligner_0_6b",
    "rmvpe",
    "game_1_0_3_medium",
    "basic_pitch",
    "jbm555_cectc_80",
    "stars",
    "rosvot",
    "fcpe",
    "firered_asr2_aed",
    "bs_roformer_leap_xe90_instrumental",
    "bs_polarformer_public_instrumental",
    "melband_roformer_harmony",
    "melband_roformer_denoise_aufr33",
    "melband_roformer_dereverb_anvuew",
    "game_1_0_3_small",
    "game_1_0_3_large",
];

impl SetupTier {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Maximum, Self::Complete];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Maximum => "maximum",
            Self::Complete => "complete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.as_str() == value)
    }

    pub const fn model_ids(self) -> &'static [&'static str] {
        match self {
            Self::Standard => STANDARD_MODELS,
            Self::Maximum => MAXIMUM_MODELS,
            Self::Complete => COMPLETE_MODELS,
        }
    }
}

/// Reads each setup-guide level's models, missing models, and sizes from
/// Runtime Manager. Read-only: nothing is downloaded or changed.
pub fn setup_tier_options() -> Result<Vec<SetupTierOption>, String> {
    setup_tier_options_with_client(&runtime_client()?)
}

pub(crate) fn setup_tier_options_with_client(
    client: &RuntimeCliClient,
) -> Result<Vec<SetupTierOption>, String> {
    let details = tier_model_details(client, SetupTier::Complete)?;
    Ok(SetupTier::ALL
        .into_iter()
        .map(|tier| tier_option(tier, &details))
        .collect())
}

pub(super) fn tier_model_details(
    client: &RuntimeCliClient,
    tier: SetupTier,
) -> Result<BTreeMap<&'static str, RuntimeResourceDetailsWire>, String> {
    tier.model_ids()
        .iter()
        .map(|model_id| {
            let resource = RuntimeResourceRefWire::model(model_id)?;
            client
                .show(&resource)
                .map(|details| (*model_id, details))
                .map_err(|error| format!("{model_id}: {error}"))
        })
        .collect()
}

pub(super) fn model_present(details: &RuntimeResourceDetailsWire) -> bool {
    matches!(
        details.status.install_state,
        InstallStateWire::Installed | InstallStateWire::Legacy
    )
}

fn tier_option(
    tier: SetupTier,
    details: &BTreeMap<&'static str, RuntimeResourceDetailsWire>,
) -> SetupTierOption {
    let models = tier
        .model_ids()
        .iter()
        .filter_map(|model_id| details.get(model_id).map(|details| (*model_id, details)))
        .collect::<Vec<_>>();
    let missing = models
        .iter()
        .filter(|(_, details)| !model_present(details))
        .collect::<Vec<_>>();
    SetupTierOption {
        tier,
        model_ids: models
            .iter()
            .map(|(model_id, _)| (*model_id).to_string())
            .collect(),
        missing_model_ids: missing
            .iter()
            .map(|(model_id, _)| (*model_id).to_string())
            .collect(),
        download_bytes: missing
            .iter()
            .map(|(_, details)| details.metadata.estimated_download_bytes)
            .sum(),
        installed_bytes: models
            .iter()
            .map(|(_, details)| details.metadata.estimated_installed_bytes)
            .sum(),
    }
}
