# Community Proving with Zksync-Era

# 1. Install all required components and configurations

# Clone repository
git clone https://github.com/johnstephan/zksync-era.git

# Checkout branch
cd zksync-era && git checkout community-proving && cd ..

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"

# NVM
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash

# Reload current shell
. .bashrc

# All necessary stuff
sudo apt update -yqq
sudo apt-get install -yqq build-essential pkg-config clang lldb lld libssl-dev postgresql

# Install cmake 3.24.2
sudo apt-get install -yqq build-essential libssl-dev checkinstall zlib1g-dev libssl-dev && \
wget https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz && \
tar -xzvf cmake-3.24.2.tar.gz && \
cd cmake-3.24.2/ && \
./bootstrap && \
make && \
sudo make install && \
cd ../ && \
echo 'export PATH="/usr/local/bin:$PATH"' >> .bashrc && \
. .bashrc

# Docker
sudo apt install -yqq apt-transport-https ca-certificates curl software-properties-common && \
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo apt-key add - && \
sudo add-apt-repository "deb [arch=amd64] https://download.docker.com/linux/ubuntu focal stable" && \
apt-cache policy docker-ce && \
sudo apt install -yqq docker-ce && \
sudo usermod -aG docker $USER

# You might need to re-connect (due to usermod change).
source .bashrc
newgrp docker

# SQL tools
cargo install sqlx-cli --version 0.7.3
# Start docker.
sudo systemctl start docker

# Solidity
sudo add-apt-repository ppa:ethereum/ethereum && \
sudo apt-get update -yqq && \
sudo apt-get install -yqq solc

# Node & yarn
nvm install 18
npm install -g yarn
yarn set version 1.22.19

# Set zksync variables
echo 'export ZKSYNC_HOME="$HOME/zksync-era"' >> .bashrc && \
echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> .bashrc

# At this point, we need to reboot
sudo reboot

# Stop the postgres database we are going to use the Docker one
sudo systemctl stop postgresql


# 2. Run the prover

# Init Era
cd zksync-era
zk init

# Troubleshooting
If you get the following error:
Error: EACCES: permission denied, mkdir'/home/$USER/zksync-era/volumes/reth/data'
Then run the following command and retry:
sudo chown -R $USER:$USER volumes

If you get the following error:
Error response from daemon: driver failed programming external connectivity on endpoint zksync-era-postgres-1
Remember to shut down the postgres server and retry:
sudo systemctl stop postgresql

# Set up and compile all prover components
# This will take close to an hour
cd prover
./setup.sh

# Now, everything is ready to run the prover/client
zk f cargo run --release --bin client