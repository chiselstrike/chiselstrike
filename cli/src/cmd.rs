// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

#[macro_export]
macro_rules! execute {
    ( $cmd:expr ) => {{
        $cmd.map_err(|x| anyhow!(x.message().to_owned()))?
            .into_inner()
    }};
}

pub(crate) mod apply;
pub(crate) mod dev;
pub(crate) mod generate;
