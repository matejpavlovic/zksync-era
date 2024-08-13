# Community Proving with Zksync-Era - Mac version

# Install all required components and configurations

# Clone repository, under community-proving branch
```bash
git clone -b community-proving https://github.com/johnstephan/zksync-era.git
```

# Set up Rust environment
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh && \
source "$HOME/.cargo/env"
```

# Install NVM and Reload current shell
```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.5/install.sh | bash && \
source ~/.zshrc  # or source ~/.bash_profile if you use bash
```

# If you're using Zsh and the .zshrc file is missing, you can create it by running
```bash
touch ~/.zshrc # or touch ~/.bash_profile if you use bash
```

# Install necessary packages
```bash
brew update && \
brew install cmake llvm pkg-config openssl && \
source ~/.zshrc
```

# Install Node & yarn
```bash
nvm install 18 && npm install -g yarn && yarn set version 1.22.19
```

# Set zksync variables
```bash
echo 'export ZKSYNC_HOME="$HOME/Desktop/zksync-era"' >> ~/.zshrc && \
echo 'export PATH="$ZKSYNC_HOME/bin:$PATH"' >> ~/.zshrc && \
source ~/.zshrc  # or source ~/.bash_profile if you use bash
```