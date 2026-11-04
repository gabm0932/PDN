## What is missing

`podnext build local` does not produce an image. `cmd_build_local`
(`crates/podnext/src/commands/mod.rs`) parses the Dockerfile, optionally executes `RUN`
(added in `2f93f2c` for #17), then computes a **layer diff of the raw context directory** as a
tar.gz, prints its digest, and deletes the tarball. Nothing is registered as an image.

So `podnext build local -t test:latest .` reports `Successfully built image 'test:latest'` while
no image exists to run.

## What would make it real

Not a small change, hence a separate issue rather than a line in #17:

1. **Pull the base image** named by `FROM` (or resolve it locally) so there is a rootfs to build on.
2. **Assemble a real rootfs**: apply the context and each instruction's filesystem effect on top of
   that base, rather than tarring the context in isolation.
3. **Produce something runnable**: register the result so `podnext run <tag>` works, and store the
   layer/rootfs where the container machinery expects it.
4. **Stop claiming success when it is not one.** Until steps 1-3 exist, the command should say what
   it actually did (produced a digest of a layer diff) rather than "Successfully built image".

`ENV` and `WORKDIR` are parsed but not applied for the same reason: there is no assembled rootfs to
apply them to. They belong in this work.

## Related

- `ARG` at the parser level: `crates/proot-distro-core/src/dockerfile.rs` still has no `Arg`
  variant (`DockerfileInstruction` + `parse_instruction`). `commands/mod.rs` works around it; the
  clean home is the parser.

Labels: enhancement
