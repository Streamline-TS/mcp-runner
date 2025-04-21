mod parser;
mod validator;

pub use parser::{Config, ServerConfig};

// For now, re-export validator functions (to be implemented)
// pub use validator::validate_config;