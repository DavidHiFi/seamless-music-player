//! The audio chain: symphonia decode, rubato resample, the EQ and gain
//! stage, the cpal output and the decode engine behind the player.

pub mod decode;
pub mod dsp;
pub mod engine;
pub mod output;
pub mod resample;
pub mod ring;
pub mod viz;
