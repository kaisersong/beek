#!/usr/bin/env python3
"""用 App Store Connect API 签发 Developer ID Application 证书，并导出 CI 用的 p12。

⚠️ 实测结论（2026-10-17）：这条路走不通。给 Apple 发 CSR 时返回
      HTTP 403 FORBIDDEN_ERROR
      "This operation can only be performed by the Account Holder."
   —— Developer ID 证书只能由**账号持有人本人**在 Xcode 或开发者后台创建，
   API Key 无论配成什么角色都不行。所以正常路径是：
     Xcode → Settings → Accounts → Manage Certificates… → + → Developer ID Application
   之后用 --export-only 从这个脚本把钥匙串里的身份导成 p12。
   本脚本剩下的用处：走 API 列一下团队现有证书、以及导出 p12。

为什么要这个脚本：CI 上的签名+公证需要一个 Developer ID 证书，而证书必须由 Apple 签发。
本机钥匙串里只有 Apple Development（开发证书，公证不收）。与其手点开发者后台，
不如用 API 按 CSR 签发 —— 私钥始终留在这台机器上，Apple 只回一份证书。

用法：
    python3 make_cert.py --issuer <ISSUER_ID> [--key-id 7UPJZVL2DU] [--p8 <path>]
                         [--dir ~/.xiaok/apple-signing]

产出（都在 --dir 下）：
    beek-developer-id.key/.csr   本机生成的私钥与 CSR（已存在则复用）
    beek-developer-id.cer        Apple 签回的证书（DER）
    beek-developer-id.p12        CI 用的打包（密码随机生成，打印在最后一行）
    login 钥匙串里也会装上这个身份（方便以后重新导出）

依赖：只要 openssl（命令行）+ Python 标准库。
"""
import argparse
import base64
import json
import os
import secrets
import struct
import tempfile
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.appstoreconnect.apple.com"


def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def der_ecdsa_to_raw(der: bytes) -> bytes:
    """把 openssl 产出的 DER ECDSA 签名转成 JWS 需要的 R||S（各 32 字节）。"""
    # DER: 30 <len> 02 <rlen> <r> 02 <slen> <s>
    if der[0] != 0x30:
        raise SystemExit("签名不是 DER SEQUENCE")
    idx = 2
    if der[1] & 0x80:  # 长长度
        n = der[1] & 0x7F
        idx = 2 + n
    def read_int(i):
        if der[i] != 0x02:
            raise SystemExit("DER 里没有 INTEGER")
        ln = der[i + 1]
        val = der[i + 2 : i + 2 + ln]
        return val.lstrip(b"\x00").rjust(32, b"\x00"), i + 2 + ln
    r, idx = read_int(idx)
    s, _ = read_int(idx)
    return r + s


def make_jwt(key_id: str, issuer: str, p8: Path, ttl: int = 1200) -> str:
    header = {"alg": "ES256", "kid": key_id, "typ": "JWT"}
    now = int(time.time())
    payload = {"iss": issuer, "iat": now, "exp": now + ttl, "aud": "appstoreconnect-v1"}
    signing_input = f"{b64url(json.dumps(header, separators=(',', ':')).encode())}." \
                    f"{b64url(json.dumps(payload, separators=(',', ':')).encode())}".encode()
    der_sig = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", str(p8)],
        input=signing_input, capture_output=True, check=True, timeout=60,
    ).stdout
    return signing_input.decode() + "." + b64url(der_ecdsa_to_raw(der_sig))


def api(token: str, method: str, path: str, body=None):
    req = urllib.request.Request(
        API + path,
        data=json.dumps(body).encode() if body else None,
        method=method,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.loads(resp.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")
        raise SystemExit(f"API {method} {path} 失败：HTTP {e.code}\n{detail}")


def pack_p12_from_cer(workdir: Path, p12: Path) -> None:
    """从开发者后台下载的 .cer + 本机私钥打包出 CI 用的 p12。

    为什么不用 `security export -t identities`：那会把登录钥匙串里**所有**身份一起导出，
    等于把 Apple Development 私钥也塞进 CI 的 secret。这里只打包这张证书 + 它的私钥 + 中间证书。
    """
    key = workdir / "beek-developer-id.key"
    cer = workdir / "beek-developer-id.cer"
    cert_pem = workdir / "beek-developer-id.cert.pem"
    chain_pem = workdir / "developer-id-ca.pem"
    if not cer.exists():
        raise SystemExit(f"没有证书文件：{cer}（从开发者后台下载 Developer ID Application 证书后放这里）")
    subprocess.run(["openssl", "x509", "-inform", "DER", "-in", str(cer),
                    "-out", str(cert_pem)], check=True, timeout=60)
    # 中间证书：从系统钥匙串里取 Developer ID Certification Authority
    chain = subprocess.run(["security", "find-certificate", "-a", "-c",
                            "Developer ID Certification Authority", "-p"],
                           capture_output=True, text=True, timeout=60).stdout
    certfile_args = []
    if "BEGIN CERTIFICATE" in chain:
        chain_pem.write_text(chain)
        certfile_args = ["-certfile", str(chain_pem)]
        print("   已带上中间证书 Developer ID Certification Authority")
    else:
        print("   ⚠️ 系统钥匙串里没找到中间证书，p12 里只有叶子证书（CI 上可能链不完整）")

    print("== 把证书 + 私钥装进登录钥匙串（方便以后核对/重导）==")
    login = str(Path.home() / "Library/Keychains/login.keychain-db")
    # 已经装过会报 "item already exists"，这不是错（重复跑要能过），后面用
    # find-identity 确认身份真的在就行
    for src in (str(key), str(cer)):
        r = subprocess.run(["security", "import", src, "-k", login,
                            "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"],
                           capture_output=True, text=True, timeout=120)
        msg = (r.stdout or r.stderr).strip().splitlines()
        print("   " + (msg[0] if msg else f"（{src.split('/')[-1]} 无输出）"))
    listed = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"],
                            capture_output=True, text=True, timeout=60).stdout
    identity = next((l.strip() for l in listed.splitlines()
                     if "Developer ID Application" in l and "valid identities" not in l), None)
    print(f"   钥匙串身份：{identity.split('"')[1] if identity else '⚠️ 看不到，仍继续打包'}")

    print("== 打包 p12（只含这张证书）==")
    p12_password = secrets.token_urlsafe(24)
    if p12.exists():
        p12.unlink()
    # -legacy 不是可选项：OpenSSL 3 默认用 AES-256 + PBKDF2 打包 PKCS#12，
    # 而 macOS 的 Security.framework 只认老的 RC2/3DES 格式，导入时会报
    # 「MAC verification failed during PKCS12 import (wrong password?)」——
    # 看起来像密码错，其实是格式不认。CI 上就是这么翻的车，本机已复现过。
    subprocess.run(["openssl", "pkcs12", "-export", "-legacy", "-inkey", str(key),
                    "-in", str(cert_pem), *certfile_args,
                    "-out", str(p12), "-passout", f"pass:{p12_password}",
                    "-name", "Developer ID Application"], check=True, timeout=120)
    os.chmod(p12, 0o600)
    print(f"   {p12}（{p12.stat().st_size} 字节）")

    # 自检：用 CI 会用到的同一条命令，在临时钥匙串里试一次导入。
    # 打包格式不对在这里就暴露，不必等 CI 跑十分钟才发现。
    print("== 自检：用 CI 的同一条 security import 试导入 ==")
    probe = Path(tempfile.mkdtemp()) / "probe.keychain-db"
    probe_pw = secrets.token_hex(12)
    try:
        subprocess.run(["security", "create-keychain", "-p", probe_pw, str(probe)],
                       check=True, capture_output=True, timeout=60)
        subprocess.run(["security", "unlock-keychain", "-p", probe_pw, str(probe)],
                       check=True, capture_output=True, timeout=60)
        imported = subprocess.run(
            ["security", "import", str(p12), "-k", str(probe), "-P", p12_password,
             "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"],
            capture_output=True, text=True, timeout=120)
        identities = subprocess.run(
            ["security", "find-identity", "-v", "-p", "codesigning", str(probe)],
            capture_output=True, text=True, timeout=60).stdout
        ok = "Developer ID Application" in identities
        print("   " + (imported.stdout or imported.stderr).strip()[:120])
        if not ok:
            raise SystemExit("❌ 这一步不打过就别写 secrets：导入后找不到 Developer ID 身份\n"
                             + identities)
        print("   ✅ CI 那条导入命令能拿到 Developer ID 身份")
    finally:
        subprocess.run(["security", "delete-keychain", str(probe)],
                       capture_output=True, timeout=60)
    print(f"P12_PASSWORD={p12_password}")


def export_identity(workdir: Path, p12: Path) -> None:
    """把钥匙串里已有的 Developer ID 身份导出成 CI 用的 p12（密码随机，打印在最后一行）。"""
    listed = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"],
                            capture_output=True, text=True, timeout=60).stdout
    identity = next((l.strip() for l in listed.splitlines()
                     if "Developer ID Application" in l and "valid identities" not in l), None)
    if not identity:
        raise SystemExit("钥匙串里没有 Developer ID Application 身份 —— 先用 Xcode 或开发者后台建一个")
    print("身份：" + identity.split('"')[1])
    p12_password = secrets.token_urlsafe(24)
    if p12.exists():
        p12.unlink()
    subprocess.run(["security", "export", "-k",
                    str(Path.home() / "Library/Keychains/login.keychain-db"),
                    "-t", "identities", "-f", "pkcs12", "-P", p12_password,
                    "-o", str(p12)], check=True, timeout=180)
    os.chmod(p12, 0o600)
    print(f"p12：{p12}（{p12.stat().st_size} 字节）")
    print(f"P12_PASSWORD={p12_password}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--from-cer", action="store_true",
                    help="用开发者后台下载的 beek-developer-id.cer + 本机私钥打包 p12")
    ap.add_argument("--export-only", action="store_true",
                    help="跳过 API，直接把钥匙串里已有的 Developer ID 身份导出成 p12")
    ap.add_argument("--issuer", help="App Store Connect 的 Issuer ID（只有走 API 时才需要）")
    ap.add_argument("--key-id", help="App Store Connect API Key 的 Key ID（只有走 API 时才需要）")
    ap.add_argument("--p8", help="该 Key 的 .p8 文件路径（只有走 API 时才需要）")
    ap.add_argument("--dir", default=str(Path.home() / ".xiaok/apple-signing"))
    args = ap.parse_args()

    workdir = Path(args.dir).expanduser()
    workdir.mkdir(parents=True, exist_ok=True)
    os.chmod(workdir, 0o700)
    key = workdir / "beek-developer-id.key"
    csr = workdir / "beek-developer-id.csr"
    cer = workdir / "beek-developer-id.cer"
    p12 = workdir / "beek-developer-id.p12"

    if not csr.exists():
        raise SystemExit(f"没有 CSR：{csr}（先用 openssl 生成私钥和 CSR）")

    if args.from_cer:
        return pack_p12_from_cer(workdir, p12)
    if args.export_only:
        return export_identity(workdir, p12)

    for need in ("issuer", "key_id", "p8"):
        if not getattr(args, need):
            raise SystemExit(f"走 API 时必须给 --{need.replace('_','-')}")
    token = make_jwt(args.key_id, args.issuer, Path(args.p8).expanduser())

    print("== 1/4 问 API：团队已有的 Developer ID Application 证书 ==")
    listed = api(token, "GET", "/v1/certificates?limit=200")
    for c in listed.get("data", []):
        attrs = c.get("attributes", {})
        if attrs.get("certificateType") == "DEVELOPER_ID_APPLICATION":
            print(f"   {c['id']}  {attrs.get('displayName')}  到期 {attrs.get('expirationDate')}")

    print("== 2/4 用本机 CSR 签发新证书 ==")
    created = api(token, "POST", "/v1/certificates", {
        "data": {
            "type": "certificates",
            "attributes": {
                "certificateType": "DEVELOPER_ID_APPLICATION",
                "csrContent": csr.read_text(),
            },
        }
    })
    data = created["data"]
    print(f"   已签发：{data['id']}  {data['attributes'].get('displayName')}")

    print("== 3/4 取回证书内容并写入钥匙串 ==")
    content = data["attributes"]["certificateContent"]
    cer.write_bytes(base64.b64decode(content))
    os.chmod(cer, 0o600)
    # 私钥 + 证书凑成一个身份；允许 codesign/security 免交互使用（导出 p12 需要）
    subprocess.run(["security", "import", str(cer), "-k",
                    str(Path.home() / "Library/Keychains/login.keychain-db"),
                    "-T", "/usr/bin/codesign", "-T", "/usr/bin/security", "-T", "/usr/bin/security"],
                   check=True, timeout=120)
    subprocess.run(["security", "import", str(key), "-k",
                    str(Path.home() / "Library/Keychains/login.keychain-db"),
                    "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"],
                   check=True, timeout=120)
    listed = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"],
                            capture_output=True, text=True, timeout=60).stdout
    identity = next((l.strip() for l in listed.splitlines() if "Developer ID Application" in l), None)
    if not identity:
        raise SystemExit("钥匙串里还是看不到 Developer ID Application 身份")
    print(f"   身份就绪：{identity.split('\"')[1]}")

    print("== 4/4 导出 p12（给 CI 用）==")
    p12_password = secrets.token_urlsafe(24)
    if p12.exists():
        p12.unlink()
    subprocess.run(["security", "export", "-k",
                    str(Path.home() / "Library/Keychains/login.keychain-db"),
                    "-t", "identities", "-f", "pkcs12", "-P", p12_password,
                    "-o", str(p12)], check=True, timeout=180)
    os.chmod(p12, 0o600)
    size = p12.stat().st_size
    print(f"   {p12}（{size} 字节）")
    print(f"P12_PASSWORD={p12_password}")


if __name__ == "__main__":
    main()
