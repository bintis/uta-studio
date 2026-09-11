use serde_json::Value;
use uta_ggml_runtime::qwen::model::Metadata;

pub(super) struct NativeMetadata<'a>(pub &'a Value);
impl NativeMetadata<'_> {
    fn field(&self, key: &str) -> Result<&Value, String> {
        self.0
            .get(key)
            .ok_or_else(|| format!("native Qwen metadata is missing {key}"))
    }
}
impl Metadata for NativeMetadata<'_> {
    fn required_string(&self, key: &str) -> Result<String, String> {
        self.field(key)?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("native Qwen metadata is not a string: {key}"))
    }
    fn required_usize(&self, key: &str) -> Result<usize, String> {
        self.field(key)?
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| format!("native Qwen metadata is not an addressable integer: {key}"))
    }
    fn required_f32(&self, key: &str) -> Result<f32, String> {
        self.field(key)?
            .as_f64()
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("native Qwen metadata is not finite numeric data: {key}"))
    }
    fn string_array(&self, key: &str) -> Result<Vec<String>, String> {
        self.field(key)?
            .as_array()
            .ok_or_else(|| format!("native Qwen metadata is not an array: {key}"))?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    format!("native Qwen metadata array contains a non-string: {key}")
                })
            })
            .collect()
    }
    fn i32_array(&self, key: &str) -> Result<Vec<i32>, String> {
        self.field(key)?
            .as_array()
            .ok_or_else(|| format!("native Qwen metadata is not an array: {key}"))?
            .iter()
            .map(|value| {
                value
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| {
                        format!("native Qwen metadata array contains an invalid integer: {key}")
                    })
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_metadata_preserves_text_arrays_and_numeric_types() {
        let json = serde_json::json!({"name":"Qwen", "count":12,"epsilon":0.00001,
            "tokens":["你","<timestamp>"],"types":[1,4],"invalid":-1});
        let metadata = NativeMetadata(&json);
        assert_eq!(metadata.required_string("name").unwrap(), "Qwen");
        assert_eq!(metadata.required_usize("count").unwrap(), 12);
        assert_eq!(metadata.required_f32("epsilon").unwrap(), 0.00001);
        assert_eq!(
            metadata.string_array("tokens").unwrap(),
            ["你", "<timestamp>"]
        );
        assert_eq!(metadata.i32_array("types").unwrap(), [1, 4]);
        assert!(metadata.required_usize("invalid").is_err());
        assert!(metadata.required_string("count").is_err());
        assert!(metadata.i32_array("tokens").is_err());
        assert!(metadata.required_usize("missing").is_err());
    }
}
