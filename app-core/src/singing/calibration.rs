use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalibrationMethod {
    Identity,
    Platt { slope: f32, bias: f32 },
    Temperature { temperature: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreCalibrator {
    pub version: String,
    pub method: CalibrationMethod,
}

impl ScoreCalibrator {
    pub fn calibrate(&self, raw_score: f32) -> Result<f32, String> {
        if !raw_score.is_finite() {
            return Err("expert score is not finite".to_string());
        }
        let probability = match self.method {
            CalibrationMethod::Identity => raw_score,
            CalibrationMethod::Platt { slope, bias } => {
                if !slope.is_finite() || !bias.is_finite() {
                    return Err("Platt calibration parameters are not finite".to_string());
                }
                1.0 / (1.0 + (-(slope * raw_score + bias)).exp())
            }
            CalibrationMethod::Temperature { temperature } => {
                if !temperature.is_finite() || temperature <= 0.0 {
                    return Err("calibration temperature must be positive".to_string());
                }
                let probability = raw_score.clamp(1.0e-6, 1.0 - 1.0e-6);
                let logit = (probability / (1.0 - probability)).ln() / temperature;
                1.0 / (1.0 + (-logit).exp())
            }
        };
        Ok(probability.clamp(0.0, 1.0))
    }
}
