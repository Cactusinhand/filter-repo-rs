use super::SanityCheckError;
use crate::opts::Options;

pub struct SensitiveModeValidator;

impl SensitiveModeValidator {
    pub fn validate_options(opts: &Options) -> Result<(), SanityCheckError> {
        if !opts.sensitive {
            return Ok(());
        }
        if opts.force {
            return Ok(());
        }
        Self::check_stream_override_compatibility(opts)?;
        Self::check_source_target_compatibility(opts)?;
        Ok(())
    }

    fn check_stream_override_compatibility(opts: &Options) -> Result<(), SanityCheckError> {
        if opts.fe_stream_override.is_some() {
            return Err(SanityCheckError::SensitiveDataIncompatible {
                option: "--fe_stream_override".to_string(),
                suggestion: "Remove --fe_stream_override when using --sensitive mode, or use separate operations".to_string(),
            });
        }
        Ok(())
    }

    fn check_source_target_compatibility(opts: &Options) -> Result<(), SanityCheckError> {
        let default_opts = Options::default();
        if opts.source != default_opts.source {
            return Err(SanityCheckError::SensitiveDataIncompatible {
                option: format!("--source {}", opts.source.display()),
                suggestion: "Use default source path (current directory) when in --sensitive mode"
                    .to_string(),
            });
        }
        if opts.target != default_opts.target {
            return Err(SanityCheckError::SensitiveDataIncompatible {
                option: format!("--target {}", opts.target.display()),
                suggestion: "Use default target path (current directory) when in --sensitive mode"
                    .to_string(),
            });
        }
        Ok(())
    }
}
