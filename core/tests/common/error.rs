

use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum KoreError {
    #[error("Kore Start Error {0}")]
    StartError(kore_base::Error),
    
}
