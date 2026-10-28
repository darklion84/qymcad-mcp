# Python 3 stdlib only; reproduces glyph-gap.ttf. Synthetic test glyphs, no third-party font data.
import struct
import zlib
import binascii
from pathlib import Path
u16=lambda *v: struct.pack('>'+'H'*len(v),*v)
i16=lambda *v: struct.pack('>'+'h'*len(v),*v)
u32=lambda *v: struct.pack('>'+'I'*len(v),*v)
head=u32(0x10000,0,0,0x5f0f3cf5)+u16(0,1000)+bytes(16)+i16(0,0,600,700)+u16(0,8)+i16(2,0,0)
hhea=u32(0x10000)+i16(800,-200,0)+u16(600)+i16(0,0,600,1,0,0,0,0,0,0,0)+u16(3)
glyph=i16(1,0,0,600,700)+u16(3,0)+bytes([1]*4)+i16(0,600,0,-600)+i16(0,0,700,0)
fmt4=u16(4,32,0,4,4,1,0,66,65535,0,65,65535)+i16(-64,1)+u16(0,0)
name='Review Glyph Gap'.encode('utf-16-be')
tables={b'head':head,b'hhea':hhea,b'maxp':u32(0x10000)+u16(3)+bytes(26),b'hmtx':(u16(600)+i16(0))*3,b'loca':u16(0,0,len(glyph)//2,len(glyph)//2),b'glyf':glyph,b'cmap':u16(0,1,3,1)+u32(12)+fmt4,b'name':u16(0,1,18,3,1,0x409,1,len(name),0)+name}
# B is a real bitmap glyph (sbix), so a renderer can draw it, but the native outliner cannot.
def chunk(tag,data):
    return u32(len(data))+tag+data+u32(binascii.crc32(tag+data)&0xffffffff)
png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',u32(1,1)+bytes([8,6,0,0,0]))+chunk(b'IDAT',zlib.compress(bytes([0,0,0,0,255])))+chunk(b'IEND',b'')
bitmap=i16(0,0)+b'png '+png
strike=u16(16,72)+u32(20,20,20,20+len(bitmap))+bitmap
tables[b'sbix']=u16(1,1)+u32(1,12)+strike
entries=[]; data=bytearray(); pos=12+16*len(tables)
for tag,content in sorted(tables.items()):
    padded=content+bytes((-len(content))%4)
    checksum=sum(struct.unpack('>'+'I'*(len(padded)//4),padded))&0xffffffff
    entries.append(tag+u32(checksum,pos,len(content)))
    data+=padded;pos+=len(padded)
Path(__file__).with_name('glyph-gap.ttf').write_bytes(u32(0x10000)+u16(len(tables),128,3,16)+b''.join(entries)+data)
