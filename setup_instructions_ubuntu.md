# Community Proving with Zksync-Era - Ubuntu Version

## Introduction
This guide will assist you in setting up the necessary environment on an Ubuntu machine to participate in community proving with Zksync-Era.

## Prerequisites
- Ubuntu 20.04 or later
- Basic knowledge of terminal usage

## 1. Clone the Repository
Start by cloning the repository under the `community-proving` branch:
```bash
git clone -b community-proving https://github.com/johnstephan/zksync-era.git
```

## 2. Set Up Rust Environment
Install Rust by running the following command:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && . "$HOME/.cargo/env"
```

## 3. Install NVM and Reload Shell
Install Node Version Manager (NVM) and reload your shell:
```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash && . ~/.bashrc
```

## 4. Install Necessary Packages
Update your package lists and install the required packages:
```bash
sudo apt update -yqq && sudo apt-get install -yqq build-essential pkg-config clang lldb lld libssl-dev
```

## 5. Install CMake 3.24.2
Install CMake version 3.24.2 by running the following commands:
```bash
sudo apt-get install -yqq build-essential libssl-dev checkinstall zlib1g-dev libssl-dev && wget https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz && tar -xzvf cmake-3.24.2.tar.gz && cd cmake-3.24.2/ && ./bootstrap && make && sudo make install && cd ../ && echo 'export PATH="/usr/local/bin:$PATH"' >> ~/.bashrc && . ~/.bashrc
```

## 6. Install Node & Yarn
Install Node.js version 18 and Yarn:
```bash
nvm install 18 && npm install -g yarn && yarn set version 1.22.19
```

## 7. Set Zksync Variables
Set up environment variables for Zksync:
```bash
echo 'export ZKSYNC_HOME="$HOME/zksync-era"' >> ~/.bashrc && echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> ~/.bashrc
```

## 8. Reboot Your System
Finally, reboot your system to apply all changes:
```bash
sudo reboot
```

## Final Steps
Your Ubuntu environment is now set up for community proving with Zksync-Era. You can proceed with running the prover as instructed in the main README.