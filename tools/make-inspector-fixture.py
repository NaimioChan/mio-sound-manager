"""Build a small synthetic library for the details/cover acceptance run.

    python tools/make-inspector-fixture.py C:\\Temp\\mio-inspector-lib

The real library must never be written to by a development pass (see
`docs/details-panel-todo.md` §11), so the desktop checks run against this folder
instead. It writes:

* a WAV with a BWF `bext` chunk, an `iXML` chunk, a `LIST/INFO` block and an
  embedded ID3v2 tag carrying a front-cover APIC;
* a WAV with no tags and a `cover.png` sidecar in the same folder;
* three plain WAVs with no picture at all (the normal case).

No third-party dependency: PNG bytes are built with `zlib`, the ID3 tag is
written frame by frame, and the WAVs are silent.
"""
import struct
import sys
import zlib
from pathlib import Path


def png(width: int, height: int, rgb: tuple[int, int, int]) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    row = bytes(rgb) * width
    raw = b"".join(b"\x00" + row for _ in range(height))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def wav(frames: int = 44_100 * 5, rate: int = 44100, channels: int = 2) -> bytes:
    block = channels * 2
    data = b"\x00\x00" * (frames * channels)
    fmt = struct.pack("<HHIIHH", 1, channels, rate, rate * block, block, 16)
    body = b"WAVE" + b"fmt " + struct.pack("<I", len(fmt)) + fmt
    body += b"data" + struct.pack("<I", len(data)) + data
    return b"RIFF" + struct.pack("<I", len(body)) + body


def riff_chunk(kind: bytes, payload: bytes) -> bytes:
    chunk = kind + struct.pack("<I", len(payload)) + payload
    if len(payload) & 1:
        chunk += b"\x00"
    return chunk


def bext(description: str, originator: str, date: str) -> bytes:
    body = bytearray(602)
    body[0 : len(description)] = description.encode("ascii")
    body[256 : 256 + len(originator)] = originator.encode("ascii")
    body[320 : 320 + len(date)] = date.encode("ascii")
    body[330:338] = b"10:42:07"
    body += b"A=PCM,F=44100,W=16,T=Mio inspector fixture\r\n"
    return bytes(body)


def ixml(project: str, scene: str, take: str) -> bytes:
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        f"<BWFXML><PROJECT>{project}</PROJECT><SCENE>{scene}</SCENE>"
        "<STEINBERG><ATTR_LIST><ATTR><NAME>TAKE</NAME>"
        f"<VALUE>{take}</VALUE></ATTR></ATTR_LIST></STEINBERG></BWFXML>"
    ).encode("utf-8")


def list_info(artist: str, title: str, comment: str) -> bytes:
    body = b"INFO"
    for code, value in ((b"IART", artist), (b"INAM", title), (b"ICMT", comment)):
        payload = value.encode("ascii")
        body += code + struct.pack("<I", len(payload)) + payload
        if len(payload) & 1:
            body += b"\x00"
    return body


def id3_frame(frame_id: bytes, payload: bytes) -> bytes:
    return frame_id + struct.pack(">I", len(payload)) + b"\x00\x00" + payload


def id3_text(frame_id: bytes, text: str) -> bytes:
    # Encoding 3 = UTF-8, then the string.
    return id3_frame(frame_id, b"\x03" + text.encode("utf-8"))


def id3_apic(image: bytes, mime: str = "image/png") -> bytes:
    # Encoding 3 (UTF-8), mime, picture type 3 (front cover), empty description,
    # then the image itself.
    payload = b"\x03" + mime.encode("ascii") + b"\x00" + b"\x03" + b"\x00" + image
    return id3_frame(b"APIC", payload)


def id3_tag(frames: bytes) -> bytes:
    # Synchsafe size, version 2.3.
    size = len(frames)
    synchsafe = bytes(
        ((size >> 21) & 0x7F, (size >> 14) & 0x7F, (size >> 7) & 0x7F, size & 0x7F)
    )
    return b"ID3\x03\x00\x00" + synchsafe + frames


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "mio-inspector-lib")
    root.mkdir(parents=True, exist_ok=True)

    cover = png(320, 320, (196, 90, 40))
    tag = id3_tag(
        id3_text(b"TIT2", "Slam On The Docks")
        + id3_text(b"TPE1", "Naimio Field Unit")
        + id3_text(b"TALB", "Harbour Sessions")
        + id3_text(b"TCON", "Foley")
        + id3_text(b"TCON", "Foley")
        + id3_text(b"COMM", "Recorded at pier 7 with the door half open.")
        + id3_apic(cover)
    )
    tagged = (
        wav()
        + riff_chunk(b"bext", bext("Dock door slam, take 3", "Naimio Field Unit", "2026-11-28"))
        + riff_chunk(b"iXML", ixml("Harbour Sessions", "Pier 7", "3"))
        + riff_chunk(b"LIST", list_info("Naimio Field Unit", "Slam On The Docks", "Harbour ambience"))
        + riff_chunk(b"ID3 ", tag)
    )
    (root / "Harbour").mkdir(exist_ok=True)
    (root / "Harbour" / "Door_Slam_Take3.wav").write_bytes(tagged)

    # A sidecar cover, no embedded picture and no useful tags.
    (root / "Pack_Sidecar").mkdir(exist_ok=True)
    (root / "Pack_Sidecar" / "Rain_On_Tin.wav").write_bytes(wav())
    (root / "Pack_Sidecar" / "cover.png").write_bytes(png(256, 256, (40, 90, 160)))

    # The normal case: most files have no picture at all. Twelve of them, because
    # the interaction checks want a list long enough to scroll and a clip long
    # enough to watch the playhead move.
    (root / "Pack_Plain").mkdir(exist_ok=True)
    for index in range(12):
        (root / "Pack_Plain" / f"Metal_Hit_{index:02}.wav").write_bytes(wav())
    (root / "Pack_Plain" / "Kick_Deep_01.wav").write_bytes(wav())
    (root / "Pack_Plain" / "Whoosh_03.wav").write_bytes(wav())
    # Two rows with a stated tempo, so the BPM column's sort and its
    # unmarked-rows-last rule have something real to sort.
    (root / "Pack_Plain" / "Drumloop_128BPM.wav").write_bytes(wav())
    (root / "Pack_Plain" / "Drumloop_90BPM.wav").write_bytes(wav())
    # And one nested folder whose ancestor carries the picture.
    (root / "Pack_Sidecar" / "Discs").mkdir(exist_ok=True)
    (root / "Pack_Sidecar" / "Discs" / "Rain_Far.wav").write_bytes(wav())

    print(f"fixture library written to {root}")


if __name__ == "__main__":
    main()
