#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Script to add LGPL license headers to all Rust and Proto files

set -e

RUST_LICENSE='// SPDX-License-Identifier: LGPL-2.1-or-later
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

'

PROTO_LICENSE='// SPDX-License-Identifier: LGPL-2.1-or-later
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

'

count=0

# Add license to Rust files
echo "Processing Rust files..."
find src crates -type f -name "*.rs" | while read file; do
    if ! head -1 "$file" | grep -q "SPDX-License-Identifier"; then
        echo "Adding license to: $file"
        echo "$RUST_LICENSE" | cat - "$file" > "$file.tmp" && mv "$file.tmp" "$file"
        ((count++))
    fi
done

# Add license to Proto files
echo "Processing Proto files..."
find proto -type f -name "*.proto" | while read file; do
    if ! head -1 "$file" | grep -q "SPDX-License-Identifier"; then
        echo "Adding license to: $file"
        echo "$PROTO_LICENSE" | cat - "$file" > "$file.tmp" && mv "$file.tmp" "$file"
        ((count++))
    fi
done

echo "Done! Added license headers to files."
echo "Note: Files that already had license headers were skipped."
