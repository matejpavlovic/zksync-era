# Community Proving with Zksync-Era - Ubuntu version

# Install all required components and configurations

# Clone repository, under community-proving branch
```bash
git clone -b community-proving https://github.com/johnstephan/zksync-era.git
```

# Set up Rust environment
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && \
. "$HOME/.cargo/env"
```

# Install NVM and Reload current shell
```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash && \
. ~/.bashrc
```

# Install necessary packages
```bash
sudo apt update -yqq && \
sudo apt-get install -yqq build-essential pkg-config clang lldb lld libssl-dev
```

# Install cmake 3.24.2
```bash
sudo apt-get install -yqq build-essential libssl-dev checkinstall zlib1g-dev libssl-dev && \
wget https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz && \
tar -xzvf cmake-3.24.2.tar.gz && \
cd cmake-3.24.2/ && \
./bootstrap && \
make && \
sudo make install && \
cd ../ && \
echo 'export PATH="/usr/local/bin:$PATH"' >> ~/.bashrc && \
. ~/.bashrc
```

# Install Node & yarn
```bash
nvm install 18 && npm install -g yarn && yarn set version 1.22.19
```

# Set zksync variables
```bash
echo 'export ZKSYNC_HOME="$HOME/zksync-era"' >> ~/.bashrc && \
echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> ~/.bashrc
```

# At this point, we need to reboot
```bash
sudo reboot
```