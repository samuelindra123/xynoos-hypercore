# Phase 5 artifact build

After the infrastructure PR is merged into `main`, open **Actions → Phase 5 artifact build → Run workflow**. Select `main` as the workflow branch and enter `phase5/hypercore-memory-intelligence` as `source_ref` (the default). To build an immutable revision, enter its full commit SHA instead. Run the workflow and wait for it to succeed.

Open the completed run and download **xynoos-cli-linux-x86_64** under Artifacts. Extract it, then check the source commit in `build-info.json` against the intended commit. For the audited Phase 5 revision it should be `18b35416724bd7787363078ab0d8e43f90842c60` when that exact SHA is selected. The branch may advance; use the SHA to pin it.

In the extracted directory:

```sh
sha256sum -c SHA256SUMS
file xynoos-cli
readelf -h xynoos-cli
ldd xynoos-cli
readelf --version-info xynoos-cli
chmod +x xynoos-cli
./xynoos-cli
```

The audited Phase 5 CLI does not support top-level `--help` or `--version`; the no-argument invocation is its smoke test.

On MX Linux, transfer and extract the artifact, verify its checksum, and run the commands above on the x86_64 machine. Check that its installed glibc and libraries meet the recorded requirements before running the binary. Artifact downloads may not preserve executable permissions, hence `chmod +x`.

A successful cloud build and smoke test does not validate operation, compatibility, or performance on the physical N5000; those require separate testing on that machine.
