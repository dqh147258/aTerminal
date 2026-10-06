#!/usr/bin/env python3
"""Export selected concept 01. Requires Inkscape and Pillow, not an app-build step."""
import copy
import json
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SVG = '{http://www.w3.org/2000/svg}'
SOURCE = ROOT / 'assets/branding/app-icon.svg'
RES = ROOT / 'apps/android/app/src/main/res'
CATALOG = ROOT / 'apps/ios/aTerminal/Assets.xcassets'
ANDROID = 'http://schemas.android.com/apk/res/android'
AAPT = 'http://schemas.android.com/aapt'
SCALE = 0.84  # Whole foreground fits inside the centered 66/108 adaptive safe circle.


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content + '\n')


def vector(source, monochrome=False):
    paths = []
    gradients = {g.attrib['id']: g for g in source.findall(f'.//{SVG}linearGradient')}
    for p in source.find(f'{SVG}g[@id="mark"]'):
        attrs = {'pathData': p.attrib['d'], 'fillColor': '#ffffff' if monochrome else p.get('fill', '#000000')}
        for svg, android in [('stroke', 'strokeColor'), ('stroke-width', 'strokeWidth'),
                             ('stroke-linecap', 'strokeLineCap'), ('stroke-linejoin', 'strokeLineJoin')]:
            if svg in p.attrib:
                attrs[android] = '#ffffff' if monochrome and svg == 'stroke' else p.attrib[svg]
        if p.get('fill') == 'none':
            attrs['fillColor'] = '#00000000'
        gradient = None
        if attrs['fillColor'].startswith('url('):
            gradient = gradients[attrs.pop('fillColor')[5:-1]]
        node = ET.Element('path', {f'android:{k}': v for k, v in attrs.items()})
        if gradient is not None:
            a = ET.SubElement(node, 'aapt:attr', {'name': 'android:fillColor'})
            # SVG objectBoundingBox: A path bounds x=115..397, y=120..366.
            g = ET.SubElement(a, 'gradient', {'android:startX': '115', 'android:startY': '120',
                'android:endX': str(115 + .7 * 282), 'android:endY': '366', 'android:type': 'linear'})
            for stop in gradient:
                ET.SubElement(g, 'item', {'android:offset': stop.get('offset', '0'), 'android:color': stop.get('stop-color')})
        paths.append(ET.tostring(node, encoding='unicode'))
    return f'''<vector xmlns:android="{ANDROID}" xmlns:aapt="{AAPT}" android:width="108dp" android:height="108dp" android:viewportWidth="512" android:viewportHeight="512">
    <group android:pivotX="256" android:pivotY="256" android:scaleX="{SCALE}" android:scaleY="{SCALE}">
        {''.join(paths)}
    </group>
</vector>'''


def main():
    source = ET.parse(SOURCE).getroot()
    with tempfile.TemporaryDirectory() as tmp:
        png = Path(tmp) / 'master.png'
        subprocess.run(['inkscape', str(SOURCE), '--export-type=png', '--export-width=1024', f'--export-filename={png}'], check=True)
        master = Image.open(png).convert('RGB')
        iconset = CATALOG / 'AppIcon.appiconset'
        iconset.mkdir(parents=True, exist_ok=True)
        entries = []
        for idiom, sizes in [('iphone', [(20,2),(20,3),(29,2),(29,3),(40,2),(40,3),(60,2),(60,3)]),
                             ('ipad', [(20,1),(20,2),(29,1),(29,2),(40,1),(40,2),(76,1),(76,2),(83.5,2)]),
                             ('ios-marketing', [(1024,1)])]:
            for points, scale in sizes:
                pixels = int(points * scale)
                name = f'icon-{pixels}.png'
                master.resize((pixels,pixels), Image.Resampling.LANCZOS).save(iconset / name)
                entries.append({'idiom': idiom, 'size': f'{points}x{points}', 'scale': f'{scale}x', 'filename': name})
        write(iconset / 'Contents.json', json.dumps({'images': entries, 'info': {'version':1,'author':'xcode'}}, indent=2))
        write(CATALOG / 'Contents.json', json.dumps({'info':{'version':1,'author':'xcode'}}, indent=2))
        for density, px in [('mdpi',48),('hdpi',72),('xhdpi',96),('xxhdpi',144),('xxxhdpi',192)]:
            folder = RES / f'mipmap-{density}'; folder.mkdir(parents=True,exist_ok=True)
            for rounded in [False, True]:
                mask = Image.new('L',(1024,1024)); d=ImageDraw.Draw(mask)
                if rounded: d.ellipse((16,16,1008,1008),fill=255)
                else: d.rounded_rectangle((16,16,1008,1008),radius=232,fill=255)
                legacy = master.convert('RGBA'); legacy.putalpha(mask)
                legacy.resize((px,px),Image.Resampling.LANCZOS).save(folder / ('ic_launcher_round.png' if rounded else 'ic_launcher.png'))
    write(RES/'drawable/ic_launcher_foreground.xml', vector(source))
    write(RES/'drawable/ic_launcher_monochrome.xml', vector(source, monochrome=True))
    write(RES/'drawable/ic_launcher_background.xml', f'''<vector xmlns:android="{ANDROID}" xmlns:aapt="{AAPT}" android:width="108dp" android:height="108dp" android:viewportWidth="512" android:viewportHeight="512">
    <path android:pathData="M0 0H512V512H0Z">
        <aapt:attr name="android:fillColor"><gradient android:startX="0" android:startY="0" android:endX="409.6" android:endY="512" android:type="linear"><item android:offset="0" android:color="#182f48"/><item android:offset="1" android:color="#071321"/></gradient></aapt:attr>
    </path>
</vector>''')
    for api in [26,33]:
        mono = '\n    <monochrome android:drawable="@drawable/ic_launcher_monochrome" />' if api == 33 else ''
        content = f'''<adaptive-icon xmlns:android="{ANDROID}">
    <background android:drawable="@drawable/ic_launcher_background" />
    <foreground android:drawable="@drawable/ic_launcher_foreground" />{mono}
</adaptive-icon>'''
        for name in ['ic_launcher','ic_launcher_round']:
            write(RES/f'mipmap-anydpi-v{api}/{name}.xml', content)


if __name__ == '__main__':
    main()
