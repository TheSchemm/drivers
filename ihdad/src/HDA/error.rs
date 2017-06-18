use std::{fmt};
use std;
#[derive(Debug)]
pub enum Error {
	CorbRPResetTimeout,
	CorbMemoryError,	
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:?}", self)
	}
}

impl std::error::Error for Error {
	fn description(&self) -> &str {
		match self {
			&Error::CorbRPResetTimeout => "Corb RP Reset Timeout",
			&Error::CorbMemoryError => "Corb Memory Error",
		}
	}
}