# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
There are no tagged releases yet (`Cargo.toml` says `0.1.0`): consumers
pin a git revision. History is in `git log`.

## [Unreleased]

### Added

- An async client for the vast.ai API: offers, instances, ssh keys and the
  ssh proxy, with an instance example.
- `OfferQuery::including_rented`, for pricing a machine you were outbid on.
- An offer's `gpu_frac` is typed, since NVENC on driver 580 and later needs
  the whole machine.
- A preempted interruptible instance reads as outbid.

### Changed

- reqwest 0.13. TLS is still rustls, now with the aws-lc-rs provider and the
  platform certificate verifier instead of bundled webpki roots, so a Linux
  host needs its CA certificates installed.
