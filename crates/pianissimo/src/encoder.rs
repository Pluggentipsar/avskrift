//! The encoder as one ONNX Runtime session.
use anyhow::{Context, Result};
use ort::{
    session::Session,
    value::{DynValue, Tensor},
};
use std::collections::HashMap;

pub struct Encoder {
    session: Session,
}

impl Encoder {
    pub fn new(session: Session) -> Self {
        Self { session }
    }
    pub fn run(
        &mut self,
        features: Vec<f32>,
        frames: usize,
        valid: usize,
        check: &impl Fn() -> Result<()>,
    ) -> Result<HashMap<String, DynValue>> {
        check()?;
        let mut outputs = self.session.run(ort::inputs! {
            "audio_signal" => Tensor::from_array(([1, 128, frames], features))?,
            "length" => Tensor::from_array(([1], vec![valid as i64]))?,
        })?;
        let mut values = HashMap::new();
        for name in ["outputs", "encoded_lengths"] {
            values.insert(
                name.to_string(),
                outputs.remove(name).context("Missing encoder output")?,
            );
        }
        Ok(values)
    }
}
