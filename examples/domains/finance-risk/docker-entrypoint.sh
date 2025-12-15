#!/bin/sh
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Docker entrypoint script for finance-node
# Fixes volume permissions before starting the application

set -e

# Fix permissions on /data/finance after volume mount
# This is necessary because Docker volumes are owned by root
if [ -d "/data/finance" ]; then
    echo "Fixing permissions on /data/finance..."
    chown -R root:root /data/finance
    chmod -R 755 /data/finance
    echo "Permissions fixed."
fi

# Execute the main command
exec "$@"
