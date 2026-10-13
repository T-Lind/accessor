"""Render the actual pixel frames as a terminal preview and a mood sheet.

Development-only dependency: Pillow. Runtime mascot has no dependencies.
Usage: python3 scripts/render-preview.py ../preview
"""
from pathlib import Path
import json
import sys
from PIL import Image, ImageDraw, ImageFont

out = Path(sys.argv[1] if len(sys.argv) > 1 else 'preview')
data = json.loads((out / 'frames.json').read_text())
P = data['palette']
mono_path = '/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf'
sans_path = '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'
mono = ImageFont.truetype(mono_path, 16)
small = ImageFont.truetype(mono_path, 12)
label = ImageFont.truetype(mono_path, 14)
heading = ImageFont.truetype(sans_path, 28)

def pixels(image, frame, x, y, scale):
    draw = ImageDraw.Draw(image)
    for j, row in enumerate(frame):
        for i, color in enumerate(row):
            if color:
                draw.rectangle((x+i*scale,y+j*scale,x+(i+1)*scale-1,y+(j+1)*scale-1), fill=P[color])

def terminal(frame, state):
    image = Image.new('RGB',(960,640),'#0c1112')
    d = ImageDraw.Draw(image)
    d.text((36,22),'accessor',font=heading,fill=P['cream'])
    d.text((660,35),'A LITTLE BEAR. A LITTLE COMPANY.',font=small,fill=P['mint'])
    d.rounded_rectangle((28,76,932,606),radius=13,fill=P['background'],outline=P['border'],width=1)
    d.rounded_rectangle((29,77,931,115),radius=12,fill=P['surface'])
    d.rectangle((29,102,931,115),fill=P['surface'])
    for i,c in enumerate(['#db8d83','#d8b572','#72b6a0']):
        d.ellipse((46+i*21,90,55+i*21,99),fill=c)
    d.text((360,87),'accessor-mascot — terminal',font=small,fill=P['muted'])
    d.text((52,133),'accessor',font=mono,fill=P['mint'])
    d.text((52,158),'A LITTLE BEAR. A LITTLE COMPANY.',font=small,fill=P['muted'])
    d.line((52,182,908,182),fill=P['border'])
    pixels(image,frame,106,192,9)
    d.text((570,200),'THE MOOD BOARD',font=small,fill=P['muted'])
    for i,s in enumerate(data['states']):
        y=231+i*19
        active=s['id']==state['id']
        if active:
            d.rounded_rectangle((558,y-1,884,y+19),radius=4,fill=P['surface'])
        d.text((571,y),f"{'›' if active else ' '} {s['key']}  {s['name']}",font=label,fill=P['mint'] if active else P['muted'])
        if active:d.ellipse((863,y+7,868,y+12),fill=P['mint'])
    d.text((64,516),'● '+state['name'].lower()+'  / on tour',font=mono,fill=P['mint'])
    d.text((64,546),state['message'],font=mono,fill=P['cream'])
    d.text((64,580),'1–9 / a–f moods   ← → browse   space pause   t tour   q quit',font=small,fill=P['muted'])
    return image

state_by_id={s['id']:s for s in data['states']}
frames=[terminal(f['pixels'],state_by_id[f['state']]) for f in data['tour']]
# One shared palette keeps the exact terminal colours stable across frames.
palette_image=Image.new('P',(1,1))
colors=list(P.values())+['#0c1112','#db8d83','#d8b572','#72b6a0']
palette=[]
for color in colors:
    palette.extend(tuple(int(color[i:i+2],16) for i in (1,3,5)))
palette += [0]*(768-len(palette))
palette_image.putpalette(palette)
indexed=[f.quantize(palette=palette_image,dither=Image.Dither.NONE) for f in frames]
indexed[0].save(out/'accessor-bear-tour.gif',save_all=True,append_images=indexed[1:],duration=100,loop=0,optimize=True,disposal=2)
terminal(state_by_id['greeting']['frames'][4],state_by_id['greeting']).save(out/'accessor-bear-terminal.png')

tile_w,tile_h=272,298
sheet=Image.new('RGB',(tile_w*5+80,tile_h*3+142),'#0c1112')
d=ImageDraw.Draw(sheet)
d.text((40,26),'Accessor / fifteen little moods',font=heading,fill=P['cream'])
d.text((40,68),'HONEY FUR · MINT SCARF · FRIENDLY PAWS',font=label,fill=P['mint'])
for i,s in enumerate(data['states']):
    x=40+(i%5)*tile_w;y=112+(i//5)*tile_h
    d.rounded_rectangle((x,y,x+tile_w-14,y+tile_h-15),radius=8,fill=P['background'])
    d.text((x+17,y+14),f"{s['key']}  {s['name'].lower()}",font=mono,fill=P['mint'])
    pixels(sheet,s['frames'][4],x+9,y+47,6)
sheet.save(out/'accessor-bear-moods.png')
print('Rendered animated tour, terminal still, and fifteen-state sheet.')
