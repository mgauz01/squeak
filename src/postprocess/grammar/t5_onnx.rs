use std::path::Path;

use ndarray::{Array2, Array3};
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;
use tracing::info;

use crate::postprocess::grammar::engine::{GrammarError, GrammarPolisher};
const REQUIRED_ONNX_FILES: &[&str] = &[
    "encoder_model_quantized.onnx",
    "decoder_model_merged_quantized.onnx",
    "tokenizer.json",
    "config.json",
];
const DECODER_START_TOKEN_ID: i64 = 0;
const EOS_TOKEN_ID: i64 = 1;
const MAX_DECODE_LEN: usize = 128;

/// Shared T5 ONNX grammar engine (encoder + merged decoder).
pub struct T5OnnxGec {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    num_decoder_layers: usize,
    input_prefix: String,
}

impl T5OnnxGec {
    pub fn load_from_dir(
        dir: &Path,
        input_prefix: impl Into<String>,
    ) -> Result<Self, GrammarError> {
        for name in REQUIRED_ONNX_FILES {
            let path = dir.join(name);
            if !path.is_file() {
                return Err(GrammarError::Other(format!(
                    "missing grammar model file: {}",
                    path.display()
                )));
            }
        }

        let encoder_path = dir.join("encoder_model_quantized.onnx");
        let decoder_path = dir.join("decoder_model_merged_quantized.onnx");
        let tokenizer_path = dir.join("tokenizer.json");

        info!("Loading grammar encoder from {}", encoder_path.display());
        let encoder = Session::builder()
            .map_err(|e| GrammarError::Polish(e.to_string()))?
            .commit_from_file(&encoder_path)
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        info!("Loading grammar decoder from {}", decoder_path.display());
        let decoder = Session::builder()
            .map_err(|e| GrammarError::Polish(e.to_string()))?
            .commit_from_file(&decoder_path)
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            num_decoder_layers: read_num_decoder_layers(dir),
            input_prefix: input_prefix.into(),
        })
    }

    fn encode_input(&self, text: &str) -> Result<(Vec<i64>, Vec<i64>), GrammarError> {
        let prefixed = if self.input_prefix.is_empty() {
            text.to_string()
        } else {
            format!("{}{text}", self.input_prefix)
        };

        let encoding = self
            .tokenizer
            .encode(prefixed, false)
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask = vec![1i64; input_ids.len()];
        Ok((input_ids, attention_mask))
    }

    fn run_encoder(
        &mut self,
        input_ids: &[i64],
        attention_mask: &[i64],
    ) -> Result<(Tensor, Tensor), GrammarError> {
        let batch = 1usize;
        let seq_len = input_ids.len();

        let input_ids_tensor = Tensor::from_array(([batch, seq_len], input_ids.to_vec()))
            .map_err(|e| GrammarError::Polish(e.to_string()))?;
        let mask_tensor = Tensor::from_array(([batch, seq_len], attention_mask.to_vec()))
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let outputs = self
            .encoder
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => mask_tensor,
            ])
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let hidden = outputs
            .get("last_hidden_state")
            .ok_or_else(|| GrammarError::Polish("encoder missing last_hidden_state".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let (shape, data) = hidden;
        let enc_hidden = Tensor::from_array((
            (shape[0] as usize, shape[1] as usize, shape[2] as usize),
            data.to_vec(),
        ))
        .map_err(|e| GrammarError::Polish(e.to_string()))?;
        let enc_mask = Tensor::from_array(([batch, seq_len], attention_mask.to_vec()))
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        Ok((enc_hidden, enc_mask))
    }

    fn decode_tokens(&self, token_ids: &[i64]) -> Result<String, GrammarError> {
        let ids: Vec<u32> = token_ids.iter().map(|&id| id as u32).collect();
        self.tokenizer
            .decode(&ids, true)
            .map_err(|e| GrammarError::Polish(e.to_string()))
    }

    fn run_decoder_step(
        &mut self,
        decoder_input: i64,
        enc_hidden: &Tensor,
        enc_mask: &Tensor,
        past_key_values: Option<&[(Array3<f32>, Array3<f32>)]>,
        use_cache: bool,
    ) -> Result<(i64, Vec<(Array3<f32>, Array3<f32>)>), GrammarError> {
        let decoder_ids = Tensor::from_array(([1usize, 1usize], vec![decoder_input]))
            .map_err(|e| GrammarError::Polish(e.to_string()))?;
        let use_cache_branch =
            Tensor::from_array(([1usize], vec![if use_cache { 1i64 } else { 0i64 }]))
                .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let mut inputs = ort::inputs![
            "input_ids" => decoder_ids,
            "encoder_attention_mask" => enc_mask,
            "encoder_hidden_states" => enc_hidden,
            "use_cache_branch" => use_cache_branch,
        ];

        if use_cache {
            let past = past_key_values.ok_or_else(|| {
                GrammarError::Polish("decoder cache branch requested without past keys".into())
            })?;
            for (layer, (key, value)) in past.iter().enumerate() {
                inputs.push((
                    format!("past_key_values.{layer}.decoder.key").into(),
                    Tensor::from_array((key.dim(), key.iter().copied().collect::<Vec<_>>()))
                        .map_err(|e| GrammarError::Polish(e.to_string()))?
                        .into(),
                ));
                inputs.push((
                    format!("past_key_values.{layer}.decoder.value").into(),
                    Tensor::from_array((value.dim(), value.iter().copied().collect::<Vec<_>>()))
                        .map_err(|e| GrammarError::Polish(e.to_string()))?
                        .into(),
                ));
            }
        }

        let outputs = self
            .decoder
            .run(inputs)
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let logits = outputs
            .get("logits")
            .ok_or_else(|| GrammarError::Polish("decoder missing logits".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| GrammarError::Polish(e.to_string()))?;

        let (logits_shape, logits_data) = logits;
        let vocab = logits_shape[logits_shape.len() - 1] as usize;
        let offset = logits_data.len().saturating_sub(vocab);
        let next_token = logits_data[offset..offset + vocab]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx as i64)
            .unwrap_or(EOS_TOKEN_ID);

        let mut next_past = Vec::with_capacity(self.num_decoder_layers);
        for layer in 0..self.num_decoder_layers {
            let key_name = format!("present.{layer}.decoder.key");
            let val_name = format!("present.{layer}.decoder.value");
            let key = outputs
                .get(&key_name)
                .or_else(|| outputs.get(&format!("present_key_values.{layer}.decoder.key")))
                .ok_or_else(|| GrammarError::Polish(format!("missing {key_name}")))?
                .try_extract_tensor::<f32>()
                .map_err(|e| GrammarError::Polish(e.to_string()))?;
            let value = outputs
                .get(&val_name)
                .or_else(|| outputs.get(&format!("present_key_values.{layer}.decoder.value")))
                .ok_or_else(|| GrammarError::Polish(format!("missing {val_name}")))?
                .try_extract_tensor::<f32>()
                .map_err(|e| GrammarError::Polish(e.to_string()))?;
            let (k_shape, k_data) = key;
            let (v_shape, v_data) = value;
            let key_arr =
                Array3::from_shape_vec((k_shape[0], k_shape[1], k_shape[2]), k_data.to_vec())
                    .map_err(|e| GrammarError::Polish(e.to_string()))?;
            let val_arr =
                Array3::from_shape_vec((v_shape[0], v_shape[1], v_shape[2]), v_data.to_vec())
                    .map_err(|e| GrammarError::Polish(e.to_string()))?;
            next_past.push((key_arr, val_arr));
        }

        Ok((next_token, next_past))
    }

    pub fn correct(&mut self, text: &str) -> Result<String, GrammarError> {
        if text.trim().is_empty() {
            return Err(GrammarError::EmptyText);
        }

        let (input_ids, attention_mask) = self.encode_input(text)?;
        let (enc_hidden, enc_mask) = self.run_encoder(&input_ids, &attention_mask)?;

        let mut generated = vec![DECODER_START_TOKEN_ID];
        let mut past_key_values: Option<Vec<(Array3<f32>, Array3<f32>)>> = None;

        for step in 0..MAX_DECODE_LEN {
            let decoder_input = *generated.last().unwrap_or(&DECODER_START_TOKEN_ID);
            let use_cache = step > 0;
            let (next_token, next_past) = self.run_decoder_step(
                decoder_input,
                &enc_hidden,
                &enc_mask,
                past_key_values.as_deref(),
                use_cache,
            )?;

            if next_token == EOS_TOKEN_ID {
                break;
            }

            generated.push(next_token);
            past_key_values = Some(next_past);
        }

        let output_ids: Vec<i64> = generated
            .into_iter()
            .filter(|&id| id != DECODER_START_TOKEN_ID)
            .collect();

        let mut text = self.decode_tokens(&output_ids)?;
        text = text.trim().to_string();
        Ok(text)
    }
}

fn read_num_decoder_layers(dir: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(dir.join("config.json")) else {
        return 4;
    };
    for key in ["num_decoder_layers", "num_layers"] {
        if let Some(value) = parse_json_u64(&content, key) {
            return value as usize;
        }
    }
    4
}

fn parse_json_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)? + needle.len();
    let tail = json[start..].trim_start();
    let tail = tail.strip_prefix(':')?.trim_start();
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    tail[..end].parse().ok()
}

impl GrammarPolisher for T5OnnxGec {
    fn is_loaded(&self) -> bool {
        true
    }

    fn polish(&mut self, text: &str) -> Result<String, GrammarError> {
        self.correct(text)
    }
}
