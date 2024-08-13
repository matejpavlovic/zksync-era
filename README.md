# Community Proving with Zksync-Era

## Introduction
This project allows users to perform community proving using Zksync-Era. Follow the steps below to set up the necessary environment and run the prover.

## Prerequisites
- Ubuntu 20.04+ or MacOS
- Bash shell
- wget or curl

## 1. Install Required Components
For Ubuntu users:
```bash
chmod +x install-ubuntu.sh
./install-ubuntu.sh
```

For Mac users:
```bash
chmod +x install-mac.sh
./install-mac.sh
```
> **Note**: These scripts require some manual interactions. If any issues arise, refer to the step-by-step guides [here](./setup_instructions_mac.md) for Mac and [here](./setup_instructions_ubuntu.md) for Ubuntu.

## 2. Download Proving and Verification Keys
Before running the prover, download the necessary proving and verification keys:

```bash
wget http://34.29.79.81:8000/setup_basic_1_data.bin
wget http://34.29.79.81:8000/verification_basic_1_key.json
```
Or using curl:
```bash
curl -O http://34.29.79.81:8000/setup_basic_1_data.bin
curl -O http://34.29.79.81:8000/verification_basic_1_key.json
```
Place these files in the following directory: `zksync-era/prover/vk_setup_data_generator_server_fri/data`.

## 3. Run the Prover
Once everything is set up, run the prover with the following command:
```bash
cd prover
chmod +x run_prover.sh
./run_prover.sh --server-url http://34.29.79.81:3030"
```
> **Note**: On Mac, the prover may crash unexpectedly. If it does, the script will automatically relaunch the prover.

## Additional Resources
- [Setup Instructions for Mac](./setup_instructions_mac.md)
- [Setup Instructions for Ubuntu](./setup_instructions_ubuntu.md)

## Contact
If you have any questions, feel free to reach out via email at [jst@matterlabs.dev](mailto:jst@matterlabs.dev), or open an issue on GitHub.