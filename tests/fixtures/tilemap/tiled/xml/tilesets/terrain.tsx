<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="terrain" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="../../images/tiles.png" width="64" height="64"/>
 <wangsets>
  <wangset name="ground" type="corner" tile="-1">
   <wangcolor name="grass" color="#ff00aa00" tile="0" probability="1"/>
   <wangcolor name="dirt" color="#ffaa5500" tile="-1" probability="0.5">
    <properties>
     <property name="speed" type="float" value="0.5"/>
    </properties>
   </wangcolor>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="1" wangid="0,2,0,1,0,1,0,1"/>
   <wangtile tileid="2" wangid="0,2,0,2,0,2,0,2"/>
  </wangset>
  <wangset name="walls" type="edge" tile="3">
   <wangcolor name="brick" color="#ff884422" tile="3" probability="1"/>
   <wangtile tileid="3" wangid="1,0,1,0,1,0,1,0"/>
   <wangtile tileid="4" wangid="1,0,0,0,1,0,0,0"/>
  </wangset>
  <wangset name="mix" type="mixed" tile="-1">
   <wangcolor name="a" color="#ffff0000" tile="-1" probability="1"/>
   <wangcolor name="b" color="#ff0000ff" tile="-1" probability="1"/>
   <wangtile tileid="5" wangid="1,2,1,2,1,2,1,2"/>
   <wangtile tileid="6" wangid="2,2,1,1,0,0,0,1"/>
  </wangset>
 </wangsets>
</tileset>
