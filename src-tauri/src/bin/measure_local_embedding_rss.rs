use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use tokenizers::Tokenizer;

type DynError = Box<dyn Error + Send + Sync>;

fn main() -> Result<(), DynError> {
    let mut args = env::args().skip(1);
    let model_dir = PathBuf::from(
        args.next()
            .expect("usage: measure_local_embedding_rss <model_dir> [query]"),
    );
    let query = args
        .next()
        .unwrap_or_else(|| "测试本地混合检索内存占用".to_string());

    let baseline = current_rss_kb()?;
    let runtime = EmbeddingRuntime::load(&model_dir)?;
    let device_label = if runtime.device.is_metal() {
        "metal"
    } else {
        "cpu"
    };
    let after_load = current_rss_kb()?;
    let vector = runtime.encode(&query)?;
    let vector_dim = vector.len();
    let after_encode = current_rss_kb()?;

    drop(runtime);
    thread::sleep(Duration::from_millis(500));
    let after_drop = current_rss_kb()?;

    println!(
        "{{\"baseline_kb\":{},\"after_load_kb\":{},\"after_encode_kb\":{},\"after_drop_kb\":{},\"vector_dim\":{}}}",
        baseline, after_load, after_encode, after_drop, vector_dim
    );
    println!("DEVICE={}", device_label);
    Ok(())
}

fn current_rss_kb() -> Result<usize, DynError> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().parse()?)
}

struct EmbeddingRuntime {
    tokenizer: Tokenizer,
    model: BertModel,
    device: Device,
}

impl EmbeddingRuntime {
    fn load(directory: &Path) -> Result<Self, DynError> {
        let device = Device::new_metal(0).unwrap_or(Device::Cpu);
        let config: BertConfig = serde_json::from_slice(&fs::read(directory.join("config.json"))?)?;
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))?;
        let builder =
            VarBuilder::from_pth(directory.join("pytorch_model.bin"), DType::F32, &device)?;
        let model = BertModel::load(builder, &config)?;
        Ok(Self {
            tokenizer,
            model,
            device,
        })
    }

    fn encode(&self, text: &str) -> Result<Vec<f32>, DynError> {
        let encoding = self.tokenizer.encode(text, true)?;

        let ids = encoding
            .get_ids()
            .iter()
            .take(512)
            .copied()
            .collect::<Vec<_>>();
        let type_ids = encoding
            .get_type_ids()
            .iter()
            .take(ids.len())
            .copied()
            .collect::<Vec<_>>();
        let mask = encoding
            .get_attention_mask()
            .iter()
            .take(ids.len())
            .copied()
            .collect::<Vec<_>>();

        let input_ids = Tensor::new(ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::new(type_ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let attention_mask = Tensor::new(mask.as_slice(), &self.device)?.unsqueeze(0)?;

        let cls = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?
            .i((0, 0))?;

        let norm = cls.sqr()?.sum_all()?.sqrt()?;
        Ok(cls.broadcast_div(&norm)?.to_vec1::<f32>()?)
    }
}
