# Community Proving with Zksync-Era - Mac Version

## Introduction
This guide will help you set up the necessary environment on a Mac to participate in community proving with Zksync-Era.

## Prerequisites
- macOS with Homebrew installed
- Basic knowledge of terminal usage

## 1. Clone the Repository
Start by cloning the repository under the `community-proving` branch:
```bash
git clone -b community-proving https://github.com/johnstephan/zksync-era.git
```

## 2. Set Up Rust Environment
Install Rust by running the following command:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && source "$HOME/.cargo/env"
```

## 3. Install NVM and Reload Shell
Install Node Version Manager (NVM) and reload your shell:
```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash && source ~/.zshrc  # Use `source ~/.bash_profile` if you use bash
```

> **Note**: If you're using Zsh and the `.zshrc` file is missing, create it by running:
```bash
touch ~/.zshrc  # or `touch ~/.bash_profile` if you use bash
```

## 4. Install Necessary Packages
Update Homebrew and install the required packages:
```bash
brew update && brew install cmake llvm pkg-config openssl && source ~/.zshrc
```

## 5. Install Node & Yarn
Install Node.js version 18 and Yarn:
```bash
nvm install 18 && npm install -g yarn && yarn set version 1.22.19
```

## 6. Set Zksync Variables
Set up environment variables for Zksync:
```bash
echo 'export ZKSYNC_HOME="$HOME/Desktop/zksync-era"' >> ~/.zshrc && echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc  # or `source ~/.bash_profile` if you use bash
```

## Final Steps
Your Mac environment is now set up for community proving with Zksync-Era. You can proceed with running the prover as instructed in the main README.