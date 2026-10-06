#!/usr/bin/env python3
"""Check launcher resource coverage, opacity, safe zone and project wiring."""
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
RES = ROOT/'apps/android/app/src/main/res'
CAT = ROOT/'apps/ios/aTerminal/Assets.xcassets'
A = '{http://schemas.android.com/apk/res/android}'


def main():
    app = ET.parse(RES.parent/'AndroidManifest.xml').getroot().find('application')
    assert app.get(A+'icon') == '@mipmap/ic_launcher'
    assert app.get(A+'roundIcon') == '@mipmap/ic_launcher_round'
    for density, size in [('mdpi',48),('hdpi',72),('xhdpi',96),('xxhdpi',144),('xxxhdpi',192)]:
        for name in ['ic_launcher', 'ic_launcher_round']:
            image = Image.open(RES/f'mipmap-{density}/{name}.png')
            assert image.size == (size,size)
            assert image.mode == 'RGBA'
            assert image.getpixel((size//2,size//2))[3] == 255
    for api in [26,33]:
        for name in ['ic_launcher', 'ic_launcher_round']:
            root = ET.parse(RES/f'mipmap-anydpi-v{api}/{name}.xml').getroot()
            for child in root:
                assert (RES/(child.get(A+'drawable').replace('@','')+'.xml')).is_file()
            assert (root.find('monochrome') is not None) == (api == 33)
    for name in ['foreground','monochrome']:
        group = ET.parse(RES/f'drawable/ic_launcher_{name}.xml').getroot().find('group')
        sx,sy = float(group.get(A+'scaleX')),float(group.get(A+'scaleY'))
        assert group.get(A+'pivotX') == group.get(A+'pivotY') == '256'
        # A vertices/control points plus stroked prompt bounds form a conservative
        # convex hull; all fit the centered 66dp circle in a 108dp layer.
        for x,y in [(115,366),(223,142),(233,120),(277,120),(287,142),
                    (397,366),(210,247),(275,247),(210,335),(322,334)]:
            assert ((x-256)*sx)**2 + ((y-256)*sy)**2 <= (512*33/108)**2
    entries = json.loads((CAT/'AppIcon.appiconset/Contents.json').read_text())['images']
    assert len(entries) == 18
    assert {e['idiom'] for e in entries} == {'iphone','ipad','ios-marketing'}
    for entry in entries:
        size = int(float(entry['size'].split('x')[0])*int(entry['scale'][0]))
        image = Image.open(CAT/'AppIcon.appiconset'/entry['filename'])
        assert image.mode == 'RGB', 'iOS icons must not contain alpha'
        assert image.size == (size,size)
    project = (ROOT/'apps/ios/aTerminal.xcodeproj/project.pbxproj').read_text()
    # xcodeproj regenerates UUIDs; validate semantic asset/resource membership.
    files = re.findall(r'([A-F0-9]{24}) /\* Assets\.xcassets \*/ = \{([^}]+)\};', project)
    asset_ids = [ident for ident, body in files if 'isa = PBXFileReference;' in body
                 and 'path = Assets.xcassets;' in body]
    assert len(asset_ids) == 1
    builds = re.findall(r'([A-F0-9]{24}) /\* Assets\.xcassets in Resources \*/ = \{([^}]+)\};', project)
    build_ids = [ident for ident, body in builds if f'fileRef = {asset_ids[0]}' in body]
    assert len(build_ids) == 1
    resource_phases = project.split('/* Begin PBXResourcesBuildPhase section */')[1].split('/* End PBXResourcesBuildPhase section */')[0]
    assert build_ids[0] in resource_phases
    groups = project.split('/* Begin PBXGroup section */')[1].split('/* End PBXGroup section */')[0]
    assert asset_ids[0] in groups
    assert project.count('ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon;') == 2
    generator = (ROOT/'scripts/generate-ios-project.rb').read_text()
    assert "group.new_file('Assets.xcassets')" in generator
    assert "'ASSETCATALOG_COMPILER_APPICON_NAME' => 'AppIcon'" in generator
    print('PASS: 10 Android legacy PNGs, adaptive/themed XML references and safe zone, 18 iOS slots, opaque images, Xcode wiring')


if __name__ == '__main__':
    main()
