# auth

JSON Web Tokens (HS256) and signed webhook payloads, in pure Soma on `hmac_sha256` and `secure_eq`. Pure: the clock is an argument.

```soma
use auth

let token = jwt_sign(map("sub", "u1", "exp", now() + 3600), secret)
let claims = jwt_verify(token, secret, now())
let header = sign_payload(secret, body, now())            // "t=…,v1=<hex>"
verify_payload(secret, body, header, now(), 300)
base64url_encode(text)  base64url_decode(text)
```

Only HS256: a token naming another algorithm (`none` included) is `BadAlgorithm`. Kinds: `Malformed`, `BadSignature`, `Expired`, `NotYetValid`, `BadSecret`. Checked against the RFC 4648 vectors and the reference HS256 token. Not here: RS256 / ES256, password hashing.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/auth/auth.cell
```
