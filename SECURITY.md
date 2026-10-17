# Security policy

Report a vulnerability privately through a
[private advisory](https://github.com/buildc-org/buildc/security/advisories/new),
or to security@buildc.org. Do not open a public issue for it.

Every release of `buildc` is before 1.0, so a minor version may break. Only the
latest release receives fixes.

The [threat model](docs/threat-model.md) names each trust boundary of `buildc`,
the installer and `common`, the control at each one, and the risks the project
accepts. A report that a control fails is in scope. A report about a risk the
threat model accepts is in scope if it shows a cost that the model does not
state.
