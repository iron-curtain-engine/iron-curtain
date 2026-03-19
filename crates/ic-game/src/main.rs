// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

fn main() {
    if let Err(error) = ic_game::run() {
        eprintln!("failed to start ic-game bootstrap: {error}");
        std::process::exit(1);
    }
}
