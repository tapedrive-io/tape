# TAPEDRIVE
[![crates.io](https://img.shields.io/crates/v/tape-api.svg?style=flat)](https://crates.io/crates/tape-api)

**Your data, permanently recorded** — uncensorable, uneditable, and here for good.

![A29B2E3C-DD15-41D1-B223-D85740194F76](https://github.com/user-attachments/assets/0f6f86b5-c4a1-414e-9263-bfa1ff6a8123)

TAPEDRIVE makes it easy to read and write data on Solana. It's over 1,400x cheaper than using an account. It works by compressing your data into tiny on-chain proofs. A network of miners then solve challenges in parallel to secure your data. It's entirely on Solana, so there's no need for side-chains or consensus overhead. The network rewards miners with the TAPE token, capped at 7 million (decaying ~15 % per year) and aligns incentives for long-term growth.


> [!NOTE]
> The program is deployed on the Solana `devnet`, but **not** on `mainnet` yet. An audit is needed before we roll it out. Stay tuned for updates!

## Quick Start

You can install the CLI using Cargo:

```bash
cargo install tapedrive-cli
```

#### Write

```
tapedrive write <filepath>
```

```
tapedrive write -m "hello, world"
```

#### Read
```
tapedrive read <id>
```

## How It Works

Whether you're writing a message, a file, or something else, tapedrive compresses the data first, then splits it into chunks that are writen to a tape on the blockchain. Each tape stores the name, number of chunks, byte size, data hash, and the tail of the tape.

When you want to retrieve your data, tapedrive reads the tape sequentially from the blockchain to reassemble the original data.

## Prerequisites
- A Solana [keypair](https://solana.com/docs/intro/installation#create-wallet) (default: `~/.config/solana/id.json`, or use `-k <filepath>` to override).
- SOL on your cluster ([Devnet is free](https://solana.com/docs/intro/installation#airdrop-sol)).

## Contributing
Fork, PR, or suggest:
- Faster writes/reads (turbo mode).
- Encryption.

Take a look at the `Makefile` if you'd like to build or test the program localy.
