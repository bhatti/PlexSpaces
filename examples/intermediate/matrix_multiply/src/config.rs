// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Configuration for Matrix Multiplication Example

use serde::Deserialize;

/// Matrix Multiplication Configuration
#[derive(Debug, Deserialize, Default)]
pub struct MatrixMultiplyConfig {
    /// Matrix size (N×N matrices)
    #[serde(default = "default_matrix_size")]
    pub matrix_size: usize,
    /// Block size for decomposition
    #[serde(default = "default_block_size")]
    pub block_size: usize,
    /// Number of worker actors
    #[serde(default = "default_num_workers")]
    pub num_workers: usize,
}

fn default_matrix_size() -> usize { 8 }
fn default_block_size() -> usize { 2 }
fn default_num_workers() -> usize { 4 }

