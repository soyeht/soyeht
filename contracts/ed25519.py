"""Ed25519 (RFC 8032) with the standard library only, for the release script and its tests.

The release machine needs no package to sign a manifest, and the gate needs none to test one.
This follows the reference implementation of RFC 8032 section 6. It is not constant time: it
runs offline, on the machine of whoever holds the release key, over a manifest of a few
kilobytes, and never on a client. The clients verify with `ed25519-dalek` and CryptoKit, which
check every signature `verify` accepts here and the test vectors of RFC 8032.
"""

import hashlib

P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)
BASE_Y = 4 * pow(5, P - 2, P) % P


def _hash(data):
    return hashlib.sha512(data).digest()


def _recover_x(y, sign):
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P:
        return None
    return P - x if x & 1 != sign else x


BASE_X = _recover_x(BASE_Y, 0)
BASE = (BASE_X, BASE_Y, 1, BASE_X * BASE_Y % P)
IDENTITY = (0, 1, 1, 0)


def _add(a, b):
    a_, b_ = (a[1] - a[0]) * (b[1] - b[0]) % P, (a[1] + a[0]) * (b[1] + b[0]) % P
    c, d = 2 * a[3] * b[3] * D % P, 2 * a[2] * b[2] % P
    e, f, g, h = b_ - a_, d - c, d + c, b_ + a_
    return (e * f % P, g * h % P, f * g % P, e * h % P)


def _multiply(scalar, point):
    result = IDENTITY
    while scalar:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def _equal(a, b):
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0


def _compress(point):
    zinv = pow(point[2], P - 2, P)
    x, y = point[0] * zinv % P, point[1] * zinv % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(data):
    if len(data) != 32:
        return None
    y = int.from_bytes(data, "little")
    x = _recover_x(y & (2**255 - 1), y >> 255)
    return None if x is None else (x, y & (2**255 - 1), 1, x * (y & (2**255 - 1)) % P)


def _expand(seed):
    if len(seed) != 32:
        raise ValueError("an Ed25519 seed is 32 bytes")
    digest = _hash(seed)
    scalar = int.from_bytes(digest[:32], "little")
    scalar &= (1 << 254) - 8
    scalar |= 1 << 254
    return scalar, digest[32:]


def public_key(seed):
    """The 32-byte public key of a 32-byte seed."""
    return _compress(_multiply(_expand(seed)[0], BASE))


def sign(seed, message):
    """The 64-byte signature of `message` by the key of `seed`."""
    scalar, prefix = _expand(seed)
    public = _compress(_multiply(scalar, BASE))
    r = int.from_bytes(_hash(prefix + message), "little") % L
    commitment = _compress(_multiply(r, BASE))
    h = int.from_bytes(_hash(commitment + public + message), "little") % L
    return commitment + int.to_bytes((r + h * scalar) % L, 32, "little")


def is_weak(public):
    """Whether `public` is not a usable key: not a point, or a point of order 8 or less.

    With such a key a signature made of the identity point and a zero scalar verifies every
    message when the check is not strict (CryptoKit's is not), so no key like it may be trusted.
    """
    point = _decompress(public)
    return point is None or _equal(_multiply(8, point), IDENTITY)


def small_order_encodings():
    """Every 32-byte encoding of a point of order 8 or less, with the forms that name `y + p`
    and the ones with the sign bit set, which a lenient decoder accepts. Sorted, as bytes."""
    for y in range(2, P):
        found = _recover_x(y, 0)
        if found is None:
            continue
        torsion = _multiply(L, (found, y, 1, found * y % P))
        if not _equal(_multiply(4, torsion), IDENTITY):
            break
    points = [IDENTITY]
    for _ in range(7):
        points.append(_add(points[-1], torsion))
    found = set()
    for point in points:
        zinv = pow(point[2], P - 2, P)
        y = point[1] * zinv % P
        for value in (y, y + P):
            if value < 2**255:
                for sign in (0, 1):
                    found.add(int.to_bytes(value | (sign << 255), 32, "little"))
    return sorted(found)


def verify(public, message, signature):
    """Whether `signature` is valid; a scalar at or above the group order and a weak key are refused."""
    if len(public) != 32 or len(signature) != 64 or is_weak(public):
        return False
    point, commitment = _decompress(public), _decompress(signature[:32])
    s = int.from_bytes(signature[32:], "little")
    if commitment is None or s >= L:
        return False
    h = int.from_bytes(_hash(signature[:32] + public + message), "little") % L
    return _equal(_multiply(s, BASE), _add(commitment, _multiply(h, point)))
