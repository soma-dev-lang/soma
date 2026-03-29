#!/bin/sh
# Redirect to the latest installer from GitHub (bypasses CDN cache)
exec curl -fsSL "https://raw.githubusercontent.com/soma-dev-lang/soma/main/site/setup.sh" | sh
