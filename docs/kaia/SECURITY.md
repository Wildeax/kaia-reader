# Security policy

## Report a vulnerability

Report security problems privately. Do not open a public issue.

Use GitHub private vulnerability reporting on Wildeax/kaia-reader. Open the Security tab of the repository and choose the option to report a vulnerability.

Include the affected commit or release, the steps to reproduce the problem and the impact you expect. Leave device serial numbers, account data and other personal information out of the report.

The project has not set a response time yet.

## Supported versions

Kaia has no release yet. No version receives security fixes. The first release will list its supported versions in this file.

## Trust model

Read this section before you install a plugin.

- Legacy plugins are trusted code. They run in KOReader's Lua environment, which can reach native FFI and operating-system services. Kaia does not sandbox them. The SSH plugin is an example of this kind of access. [S24]
- A permission manifest in the same runtime is advisory. It does not contain a plugin.
- Rust does not remove vulnerabilities in the retained C and C++ engines. These include MuPDF, CREngine, DjVuLibre and libk2pdfopt. A bug in one of these engines can still affect the reader. [S4]
- Kaia does not download plugins silently. It does not run code from book content automatically.
- A future restricted plugin mode would need its own execution environment and threat model. Kaia does not claim that such a mode protects legacy plugins.

Sources: S4 and S24 in [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md). S4: https://github.com/koreader/koreader-base . S24: https://github.com/koreader/koreader/blob/master/plugins/SSH.koplugin/main.lua
