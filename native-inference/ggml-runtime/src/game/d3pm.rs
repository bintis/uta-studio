use std::f32::consts::PI;

const STATE_SIZE: usize = 312;
const MIDDLE_WORD: usize = 156;
const MATRIX_A: u64 = 0xb502_6f5a_a966_19e9;
const UPPER_MASK: u64 = 0xffff_ffff_8000_0000;
const LOWER_MASK: u64 = 0x0000_0000_7fff_ffff;
const FLOAT_SCALE: f32 = 1.0 / ((1_u32 << 24) as f32);

pub trait RandomSource {
    fn uniform_f32(&mut self) -> Result<f32, String>;

    fn fill_uniform(&mut self, output: &mut [f32]) -> Result<(), String> {
        for value in output {
            *value = self.uniform_f32()?;
        }
        Ok(())
    }
}

/// MT19937-64 matching GAME's historical `std::mt19937_64` sampling stream.
#[derive(Clone)]
pub struct GameRng {
    state: [u64; STATE_SIZE],
    index: usize,
}

impl GameRng {
    pub fn new(seed: u64) -> Self {
        let mut state = [0_u64; STATE_SIZE];
        state[0] = seed;
        for index in 1..STATE_SIZE {
            state[index] = 6_364_136_223_846_793_005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self {
            state,
            index: STATE_SIZE,
        }
    }

    fn next_u64(&mut self) -> u64 {
        if self.index >= STATE_SIZE {
            self.twist();
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555_5555_5555_5555;
        value ^= (value << 17) & 0x71d6_7fff_eda6_0000;
        value ^= (value << 37) & 0xfff7_eee0_0000_0000;
        value ^ (value >> 43)
    }

    fn twist(&mut self) {
        for index in 0..STATE_SIZE {
            let joined = (self.state[index] & UPPER_MASK)
                | (self.state[(index + 1) % STATE_SIZE] & LOWER_MASK);
            self.state[index] = self.state[(index + MIDDLE_WORD) % STATE_SIZE]
                ^ (joined >> 1)
                ^ if joined & 1 == 0 { 0 } else { MATRIX_A };
        }
        self.index = 0;
    }
}

impl RandomSource for GameRng {
    fn uniform_f32(&mut self) -> Result<f32, String> {
        let sample = (self.next_u64() >> 40) as u32;
        Ok(sample as f32 * FLOAT_SCALE)
    }
}

pub fn d3pm_time_schedule(time: f32) -> f32 {
    0.5 * (1.0 + (time * PI).cos())
}

pub fn remove_mutable_boundaries(
    boundaries: &[u8],
    immutable: &[u8],
    probability: f32,
    random: &mut impl RandomSource,
) -> Result<Vec<u8>, String> {
    if boundaries.len() != immutable.len() {
        return Err(format!(
            "GAME boundary length mismatch: boundaries={} immutable={}",
            boundaries.len(),
            immutable.len()
        ));
    }
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err("GAME boundary removal probability is invalid".to_string());
    }
    let total = boundaries.iter().filter(|value| **value != 0).count();
    let mutable = boundaries
        .iter()
        .zip(immutable)
        .filter(|(boundary, fixed)| **boundary != 0 && **fixed == 0)
        .count();
    let drop_probability = if mutable == 0 {
        1.0
    } else {
        (total as f32 * probability / mutable as f32).min(1.0)
    };
    let mut samples = vec![0.0_f32; boundaries.len()];
    random.fill_uniform(&mut samples)?;
    Ok(boundaries
        .iter()
        .zip(immutable)
        .zip(samples)
        .map(|((boundary, fixed), sample)| {
            u8::from(*boundary != 0 && (*fixed != 0 || sample <= 1.0 - drop_probability))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InjectedRng {
        values: std::vec::IntoIter<f32>,
    }

    impl RandomSource for InjectedRng {
        fn uniform_f32(&mut self) -> Result<f32, String> {
            self.values
                .next()
                .ok_or_else(|| "injected GAME RNG exhausted".to_string())
        }
    }

    #[test]
    fn schedule_matches_reference_points() {
        assert!((d3pm_time_schedule(0.0) - 1.0).abs() < 1.0e-6);
        assert!((d3pm_time_schedule(0.5) - 0.5).abs() < 1.0e-6);
        assert!(d3pm_time_schedule(1.0).abs() < 1.0e-6);
    }

    #[test]
    fn mt19937_matches_game_seed_42() {
        let expected = [0x3f4151df, 0x3f23978f, 0x3f408c96, 0x3e0b8b10];
        let mut random = GameRng::new(42);
        for bits in expected {
            assert_eq!(random.uniform_f32().unwrap().to_bits(), bits);
        }
    }

    #[test]
    fn mutable_boundaries_rescale_probability() {
        let mut random = InjectedRng {
            values: vec![0.9, 0.8, 0.2, 0.3].into_iter(),
        };
        assert_eq!(
            remove_mutable_boundaries(&[1, 0, 1, 1], &[1, 0, 0, 0], 0.5, &mut random).unwrap(),
            vec![1, 0, 1, 0]
        );
    }

    #[test]
    fn invalid_probability_fails_closed() {
        let mut random = GameRng::new(0);
        assert!(remove_mutable_boundaries(&[1], &[0], f32::NAN, &mut random).is_err());
    }
}
