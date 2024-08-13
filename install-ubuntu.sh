#!/bin/bash

set -e # Exit immediately if any of the below commands fails

# Remember local directory as the root of community proving
ZKSYNC_HOME=$(pwd)

# Install all necessary packages
sudo apt update -y
sudo apt install -y build-essential pkg-config clang lldb lld libssl-dev postgresql checkinstall zlib1g-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Install NVM
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash

# Load NVM-related environment variables
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"  # This loads nvm
echo 'export PATH="/usr/local/bin:$PATH"' >> "$HOME/.bashrc"
export PATH="/usr/local/bin:$PATH"

# Install cmake 3.24.2
wget https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz
tar -xf cmake-3.24.2.tar.gz
cd cmake-3.24.2/
./bootstrap
make
sudo make install
cd ../

# Node & yarn
nvm install 18
npm install -g yarn
yarn set version 1.22.19

# Set zksync variables
echo "export ZKSYNC_HOME=\"$ZKSYNC_HOME\"" >> $HOME/.bashrc
echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> $HOME/.bashrc
export ZKSYNC_HOME="$ZKSYNC_HOME"
export PATH="$ZKSYNC_HOME/bin:$PATH"

# Init ZKsync Era
zk
