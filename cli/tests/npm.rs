// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

mod common;

#[cfg(test)]
mod tests {
    use crate::common::run_in;
    use std::path::Path;

    #[test]
    fn npm_build() {
        run_in(
            "npm",
            ["run", "build"],
            Path::new("../packages/chiselstrike").to_path_buf(),
        );
    }
}
