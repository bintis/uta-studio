use std::path::{Path, PathBuf};

fn partition_vocals(
    all_vocals: &[f32],
    lead_vocal: Vec<f32>,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    if all_vocals.len() != lead_vocal.len()
        || all_vocals.is_empty()
        || all_vocals.iter().any(|value| !value.is_finite())
        || lead_vocal.iter().any(|value| !value.is_finite())
    {
        return Err(
            "Harmony lead output does not preserve the finite vocal input timeline".to_string(),
        );
    }
    let vocal_residual = all_vocals
        .iter()
        .zip(&lead_vocal)
        .map(|(all, lead)| all - lead)
        .collect::<Vec<_>>();
    if vocal_residual.iter().any(|value| !value.is_finite()) {
        return Err("Harmony vocal residual is non-finite".to_string());
    }
    Ok((lead_vocal, vocal_residual))
}

pub fn infer(
    all_vocals: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<(PathBuf, PathBuf), String> {
    if config
        .get("input_semantics")
        .and_then(|value| value.as_str())
        != Some("all_vocals")
        || config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("lead_vocal+backing_vocal_residual")
    {
        return Err(
            "Harmony requires all-vocals input and explicit lead/backing residual semantics"
                .to_string(),
        );
    }
    let lead_vocal = super::melband_roformer_harmony_split::infer_pcm(
        all_vocals,
        config,
        |fraction, message| progress(fraction * 0.96, message),
    )?;
    let (lead_vocal, vocal_residual) = partition_vocals(all_vocals, lead_vocal)?;
    progress(0.97, "Atomically encoding lead vocal FLAC");
    let lead_path = crate::audio::encode_stereo_flac(&lead_vocal, output_dir, "lead-vocal.flac")?;
    progress(0.985, "Atomically encoding vocal residual FLAC");
    let residual_path =
        crate::audio::encode_stereo_flac(&vocal_residual, output_dir, "vocal-residual.flac")?;
    progress(1.0, "Lead and vocal-residual isolation complete");
    Ok((lead_path, residual_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_is_the_exact_all_vocals_minus_lead_complement() {
        let all = vec![0.75, -0.5, 0.2, 0.1];
        let lead = vec![0.5, -0.25, -0.1, 0.0];
        let (actual_lead, residual) = partition_vocals(&all, lead.clone()).unwrap();
        assert_eq!(actual_lead, lead);
        assert_eq!(residual, vec![0.25, -0.25, 0.3, 0.1]);
        for ((all, lead), residual) in all.iter().zip(actual_lead).zip(residual) {
            assert!((all - (lead + residual)).abs() < 1e-7);
        }
    }

    #[test]
    fn malformed_or_non_finite_partition_fails_closed() {
        assert!(partition_vocals(&[0.0, 0.0], vec![0.0]).is_err());
        assert!(partition_vocals(&[f32::NAN], vec![0.0]).is_err());
        assert!(partition_vocals(&[0.0], vec![f32::INFINITY]).is_err());
    }
}
