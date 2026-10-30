<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="wang" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="../../images/tiles.png" width="64" height="64"/>
 <tile id="4" probability="1"/>
 <tile id="5" probability="3"/>
 <wangsets>
  <wangset name="ground" type="corner" tile="-1">
   <wangcolor name="grass" color="#ff00aa00" tile="-1" probability="1"/>
   <wangcolor name="dirt" color="#ffaa5500" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="1" wangid="0,1,0,1,0,2,0,1"/>
   <wangtile tileid="2" wangid="0,1,0,2,0,1,0,1"/>
   <wangtile tileid="3" wangid="0,1,0,2,0,2,0,1"/>
   <wangtile tileid="4" wangid="0,2,0,1,0,1,0,1"/>
   <wangtile tileid="5" wangid="0,2,0,1,0,2,0,1"/>
   <wangtile tileid="6" wangid="0,2,0,2,0,1,0,1"/>
   <wangtile tileid="7" wangid="0,2,0,2,0,2,0,1"/>
   <wangtile tileid="8" wangid="0,1,0,1,0,1,0,2"/>
   <wangtile tileid="9" wangid="0,1,0,1,0,2,0,2"/>
   <wangtile tileid="10" wangid="0,1,0,2,0,1,0,2"/>
   <wangtile tileid="11" wangid="0,1,0,2,0,2,0,2"/>
   <wangtile tileid="12" wangid="0,2,0,1,0,1,0,2"/>
   <wangtile tileid="13" wangid="0,2,0,1,0,2,0,2"/>
   <wangtile tileid="14" wangid="0,2,0,2,0,1,0,2"/>
   <wangtile tileid="15" wangid="0,2,0,2,0,2,0,2"/>
  </wangset>
  <wangset name="walls" type="edge" tile="-1">
   <wangcolor name="brick" color="#ff884422" tile="-1" probability="1"/>
   <wangcolor name="plank" color="#ffcc9944" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="1,0,1,0,1,0,1,0"/>
   <wangtile tileid="1" wangid="1,0,1,0,1,0,2,0"/>
   <wangtile tileid="2" wangid="1,0,1,0,2,0,1,0"/>
   <wangtile tileid="3" wangid="1,0,1,0,2,0,2,0"/>
   <wangtile tileid="4" wangid="1,0,2,0,1,0,1,0"/>
   <wangtile tileid="5" wangid="1,0,2,0,1,0,2,0"/>
   <wangtile tileid="6" wangid="1,0,2,0,2,0,1,0"/>
   <wangtile tileid="7" wangid="1,0,2,0,2,0,2,0"/>
   <wangtile tileid="8" wangid="2,0,1,0,1,0,1,0"/>
   <wangtile tileid="9" wangid="2,0,1,0,1,0,2,0"/>
   <wangtile tileid="10" wangid="2,0,1,0,2,0,1,0"/>
   <wangtile tileid="11" wangid="2,0,1,0,2,0,2,0"/>
   <wangtile tileid="12" wangid="2,0,2,0,1,0,1,0"/>
   <wangtile tileid="13" wangid="2,0,2,0,1,0,2,0"/>
   <wangtile tileid="14" wangid="2,0,2,0,2,0,1,0"/>
   <wangtile tileid="15" wangid="2,0,2,0,2,0,2,0"/>
  </wangset>
  <wangset name="mix" type="mixed" tile="-1">
   <wangcolor name="a" color="#ffff0000" tile="-1" probability="1"/>
   <wangcolor name="b" color="#ff0000ff" tile="-1" probability="1"/>
   <wangcolor name="c" color="#ff00ff00" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="1,1,1,1,1,1,1,1"/>
   <wangtile tileid="1" wangid="2,2,2,2,2,2,2,2"/>
   <wangtile tileid="2" wangid="1,1,1,1,1,2,2,2"/>
   <wangtile tileid="3" wangid="1,2,2,2,1,1,1,1"/>
  </wangset>
  <wangset name="tied" type="edge" tile="-1">
   <wangcolor name="x" color="#ffffffff" tile="-1" probability="1"/>
   <wangtile tileid="4" wangid="1,0,1,0,1,0,1,0"/>
   <wangtile tileid="5" wangid="1,0,1,0,1,0,1,0"/>
  </wangset>
 </wangsets>
</tileset>
