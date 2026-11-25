//! ResNet-18 trunk that also returns the mid-layer maps.
//! candle's `resnet18_no_final_layer` only returns the pooled vector and its
//! blocks are private, so this repeats its layout with the same weight names.
use candle_core::{Result, Tensor, D};
use candle_nn::{batch_norm, conv2d_no_bias, BatchNorm, Conv2d, Conv2dConfig, VarBuilder};

const BN_EPS: f64 = 1e-5;
const BLOCKS_PER_LAYER: usize = 2;
const STEM_CHANNELS: usize = 64;
const LAYER_CHANNELS: [usize; 4] = [64, 128, 256, 512];

/// Outputs of one forward pass.
pub struct Stages {
    pub layer2: Tensor,
    pub layer3: Tensor,
    pub pooled: Tensor,
}

struct Block {
    conv1: Conv2d,
    bn1: BatchNorm,
    conv2: Conv2d,
    bn2: BatchNorm,
    downsample: Option<(Conv2d, BatchNorm)>,
}

pub struct Trunk {
    conv1: Conv2d,
    bn1: BatchNorm,
    layers: Vec<Vec<Block>>,
}

fn conv(c_in: usize, c_out: usize, k: usize, stride: usize, vb: VarBuilder) -> Result<Conv2d> {
    let cfg = Conv2dConfig {
        stride,
        padding: k / 2,
        ..Default::default()
    };
    conv2d_no_bias(c_in, c_out, k, cfg, vb)
}

impl Block {
    fn load(c_in: usize, c_out: usize, stride: usize, vb: VarBuilder) -> Result<Self> {
        let downsample = if stride != 1 || c_in != c_out {
            let vb = vb.pp("downsample");
            Some((
                conv(c_in, c_out, 1, stride, vb.pp(0))?,
                batch_norm(c_out, BN_EPS, vb.pp(1))?,
            ))
        } else {
            None
        };
        Ok(Self {
            conv1: conv(c_in, c_out, 3, stride, vb.pp("conv1"))?,
            bn1: batch_norm(c_out, BN_EPS, vb.pp("bn1"))?,
            conv2: conv(c_out, c_out, 3, 1, vb.pp("conv2"))?,
            bn2: batch_norm(c_out, BN_EPS, vb.pp("bn2"))?,
            downsample,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = xs
            .apply(&self.conv1)?
            .apply_t(&self.bn1, false)?
            .relu()?
            .apply(&self.conv2)?
            .apply_t(&self.bn2, false)?;
        let skip = match &self.downsample {
            Some((conv, bn)) => xs.apply(conv)?.apply_t(bn, false)?,
            None => xs.clone(),
        };
        (skip + ys)?.relu()
    }
}

impl Trunk {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let mut layers = Vec::with_capacity(LAYER_CHANNELS.len());
        let mut c_in = STEM_CHANNELS;
        for (i, &c_out) in LAYER_CHANNELS.iter().enumerate() {
            let stride = if i == 0 { 1 } else { 2 };
            let vb = vb.pp(format!("layer{}", i + 1));
            let mut blocks = Vec::with_capacity(BLOCKS_PER_LAYER);
            for b in 0..BLOCKS_PER_LAYER {
                let (block_in, block_stride) = if b == 0 { (c_in, stride) } else { (c_out, 1) };
                blocks.push(Block::load(block_in, c_out, block_stride, vb.pp(b))?);
            }
            layers.push(blocks);
            c_in = c_out;
        }
        Ok(Self {
            conv1: conv(3, STEM_CHANNELS, 7, 2, vb.pp("conv1"))?,
            bn1: batch_norm(STEM_CHANNELS, BN_EPS, vb.pp("bn1"))?,
            layers,
        })
    }

    pub fn forward(&self, xs: &Tensor) -> Result<Stages> {
        let mut xs = xs
            .apply(&self.conv1)?
            .apply_t(&self.bn1, false)?
            .relu()?
            .pad_with_same(D::Minus1, 1, 1)?
            .pad_with_same(D::Minus2, 1, 1)?
            .max_pool2d_with_stride(3, 2)?;
        let mut outputs = Vec::with_capacity(self.layers.len());
        for blocks in &self.layers {
            for block in blocks {
                xs = block.forward(&xs)?;
            }
            outputs.push(xs.clone());
        }
        let pooled = outputs[3].mean(D::Minus1)?.mean(D::Minus1)?;
        Ok(Stages {
            layer2: outputs[1].clone(),
            layer3: outputs[2].clone(),
            pooled,
        })
    }
}
