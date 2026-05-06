#!/usr/bin/env bash
# Downloads fonts needed for local development (not needed for Docker builds).
# The Docker build uses Debian package fonts instead.
set -euo pipefail

mkdir -p fonts
cd fonts

echo "Downloading JetBrains Mono..."
curl -fsSL "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip" \
    -o jb.zip
unzip -j jb.zip "fonts/ttf/JetBrainsMono-Regular.ttf" -d .
rm jb.zip

# Use Liberation Sans as a stand-in for Space Grotesk.
# Install it via your package manager and copy, or replace with Space Grotesk TTFs.
if command -v fc-list &>/dev/null; then
    bold_path=$(fc-list :family=LiberationSans:weight=bold | head -1 | cut -d: -f1)
    reg_path=$(fc-list :family=LiberationSans:weight=regular | head -1 | cut -d: -f1)
    [[ -n "$bold_path" ]] && cp "$bold_path" SpaceGrotesk-Bold.ttf && echo "Copied Liberation Bold"
    [[ -n "$reg_path" ]]  && cp "$reg_path"  SpaceGrotesk-Regular.ttf && echo "Copied Liberation Regular"
else
    echo "NOTE: Install fonts-liberation (Debian) or liberation-fonts-ttf (Fedora)"
    echo "      then re-run this script, or copy your own SpaceGrotesk-Bold/Regular.ttf here."
fi

echo ""
echo "Fonts ready in ./fonts/"
ls -lh
