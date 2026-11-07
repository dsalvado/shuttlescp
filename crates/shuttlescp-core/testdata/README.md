# Test data

`site-export-0.22.1.json` is a site export made by ShuttleSCP 0.22.1 (argon2 0.5.3,
chacha20poly1305 0.10.1, base64 0.22.1, getrandom 0.3.4) with the real default key
costs, passphrase `golden passphrase for tests`. Its first site's password is
`s3cret ü€ password`; the second site has none.

It is a fixed file on purpose: the test `exports_made_by_0_22_1_still_open`
(`src/site_export.rs`) opens it, so a library update that would derive keys or decrypt
differently, and leave users' existing exports unreadable, fails the tests. Never
regenerate it; add a new file for a new format instead.
