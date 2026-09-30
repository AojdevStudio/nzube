# Release preparation

Status: development and feasibility work. No release is ready for publication.

## License and content

The repository began with Apache-2.0. Keep that license for original Nzube code and original bundled guidance. Each dependency and bundled resource still requires an inventory of version, source, license, notices and distribution terms. The product license does not grant rights to imported guidance, supplied reference files, Mobbin screenshots, or private repository material.

User imports remain local user data. Development handoffs, agent state, credentials and private references must be absent from source packages, application resources, installer payloads, archives, screenshots and diagnostics. Verify this from each actual artifact's file list. Git ignore rules alone do not establish exclusion.

## Platform evidence ledger

Build, install, and exercise are separate states. An architectural test shell is not the Nzube product. Record each target independently with commit, toolchain, command, artifact hash, install location, and observed result.

| Target | Product build | Product installed | Product flow | Signing/publication |
| --- | --- | --- | --- | --- |
| macOS | Not started | Not started | Unverified | Developer ID, notarization and distribution decision pending |
| Windows | Not started | Not started | Unverified | Signing identity and installer verification pending |
| Linux | Not started | Not started | Unverified | Package formats and installed secure-storage behavior pending |
| iOS | Not started | Not started | Unverified | Apple team, bundle identifier, profiles and App Store submission pending |
| Android | Not started | Not started | Unverified | Application identifier, upload key and Play submission pending |

Do not promote these rows based on a browser preview or another platform's successful build. Mobile architectural proof results belong in the feasibility record until the actual product is built and exercised.

## Installed release check

Install outside the development checkout with isolated application data. Exercise provider connection, separate GitHub connection, guidance import and selection, intake persistence, generation, complete refinement, copy and export, restart and credential revocation. Compare clipboard and exported bytes with the full selected brief. Repeat relevant checks with cancelled generation, network interruption, expired credentials, inaccessible repository, unsupported image/provider and reference edits.

Record keyboard and screen-reader navigation, focus restoration, text scaling, narrow layouts and mobile request/preview transitions. Use a real emulator or simulator for native lifecycle and storage. Physical-device evidence and store acceptance remain separate.

## Mobile submission materials

Prepare these from the implemented product before submission:

- Store description using the tagline “Your intent, ready for your agent.” and only verified capabilities.
- Screenshots captured from the installed product on each required device class.
- Accurate privacy disclosures for provider-bound request text, repository excerpts and attachments; no claim that a provider never processes data outside the device.
- Explanation of local storage, separate provider/GitHub grants, disconnect, deletion and remote revocation.
- Provider eligibility and billing language that clearly identifies subscription and separately billed API routes, including any companion requirement.
- Review access instructions that do not embed personal accounts or credentials.
- Support and privacy-policy URLs supplied or approved by the owner; never fabricate contact values.
- Account, signing and submission steps with the precise console or command, kept separate from authorization to publish.

No submission, release upload, auto-release or website deployment is authorized by preparing these materials.
