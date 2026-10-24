"""Export the code-native star SVG to Windows icon sizes. Requires Pillow at authoring time only."""
from pathlib import Path
import io
import struct
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)

def render(size):
    shape = ET.parse(ROOT / 'assets/app-icon.svg').getroot()[0]
    points = [tuple(map(float, pair.split(','))) for pair in shape.attrib['points'].split()]
    factor = 8
    scale = size * factor / 64
    points = [(round(x * scale), round(y * scale)) for x, y in points]
    image = Image.new('RGBA', (size * factor, size * factor))
    draw = ImageDraw.Draw(image)
    draw.polygon(points, fill=shape.attrib['fill'])
    draw.line(points + [points[0]], fill=shape.attrib['stroke'],
              width=max(1, round(float(shape.attrib['stroke-width']) * scale)), joint='curve')
    return image.resize((size, size), Image.Resampling.LANCZOS)

def main():
    images = [render(size) for size in SIZES]
    payloads = []
    for image in images:
        stream = io.BytesIO()
        image.save(stream, format='PNG')
        payloads.append(stream.getvalue())
    offset = 6 + len(SIZES) * 16
    result = bytearray(struct.pack('<HHH', 0, 1, len(SIZES)))
    for size, payload in zip(SIZES, payloads):
        result += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(payload), offset)
        offset += len(payload)
    result += b''.join(payloads)
    (ROOT / 'assets/app.ico').write_bytes(result)
    images[-1].save(ROOT / 'assets/app-icon.png')
    # Review actual-size icons against both light/dark surfaces, without desktop input.
    review = Image.new('RGBA', (520, 220), '#f8f8f8')
    draw = ImageDraw.Draw(review)
    draw.rectangle((0, 110, 520, 220), fill='#101827')
    for row in (0, 110):
        x = 20
        for image in images[:7]:
            review.alpha_composite(image, (x, row + (100 - image.height) // 2))
            x += image.width + 22
    out = ROOT / '.local/icon-review.png'
    out.parent.mkdir(exist_ok=True)
    review.save(out)
    print(ROOT / 'assets/app.ico')

if __name__ == '__main__':
    main()
