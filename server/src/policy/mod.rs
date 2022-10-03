use anyhow::{bail, Result};
use std::str::FromStr;
pub mod engine;
mod instances;
mod interpreter;
pub mod type_policy;

mod utils;
#[derive(Debug)]
#[repr(u8)]
pub enum Action {
    Allow = 0,
    Deny = 1,
    Skip = 2,
    Log = 3,
}

impl TryFrom<i32> for Action {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Allow),
            1 => Ok(Self::Deny),
            2 => Ok(Self::Skip),
            3 => Ok(Self::Log),
            _ => bail!("invalid Action"),
        }
    }
}

impl Action {
    pub fn is_restrictive(&self) -> bool {
        match self {
            Action::Deny | Action::Skip => true,
            Action::Allow | Action::Log => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Location {
    UsEast1,
    UsWest,
    London,
    Germany,
}

impl FromStr for Location {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "us-east-1" => Ok(Self::UsEast1),
            "us-west" => Ok(Self::UsWest),
            "london" => Ok(Self::London),
            "germany" => Ok(Self::Germany),
            other => bail!("unknown region {other}"),
        }
    }
}
