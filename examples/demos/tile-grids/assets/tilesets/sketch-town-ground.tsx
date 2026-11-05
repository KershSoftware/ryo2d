<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.11" tiledversion="1.12.2" name="sketch-town-ground" tilewidth="256" tileheight="256" tilecount="14" columns="0">
 <tileoffset x="-12" y="116"/>
 <grid orientation="orthogonal" width="0" height="0"/>
 <tile id="0">
  <image source="../kenney-sketch-town/grass_center_N.png" width="256" height="256"/>
 </tile>
 <tile id="1">
  <image source="../kenney-sketch-town/water_center_N.png" width="256" height="256"/>
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0">
    <polygon points="128,30 244,85 128,140 12,85"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="2">
  <image source="../kenney-sketch-town/grass_water_N.png" width="256" height="256"/>
 </tile>
 <tile id="3">
  <image source="../kenney-sketch-town/grass_water_E.png" width="256" height="256"/>
 </tile>
 <tile id="4">
  <image source="../kenney-sketch-town/grass_water_S.png" width="256" height="256"/>
 </tile>
 <tile id="5">
  <image source="../kenney-sketch-town/grass_water_W.png" width="256" height="256"/>
 </tile>
 <tile id="6">
  <image source="../kenney-sketch-town/grass_waterConcave_N.png" width="256" height="256"/>
 </tile>
 <tile id="7">
  <image source="../kenney-sketch-town/grass_waterConcave_E.png" width="256" height="256"/>
 </tile>
 <tile id="8">
  <image source="../kenney-sketch-town/grass_waterConcave_S.png" width="256" height="256"/>
 </tile>
 <tile id="9">
  <image source="../kenney-sketch-town/grass_waterConcave_W.png" width="256" height="256"/>
 </tile>
 <tile id="10">
  <image source="../kenney-sketch-town/grass_waterConvex_N.png" width="256" height="256"/>
 </tile>
 <tile id="11">
  <image source="../kenney-sketch-town/grass_waterConvex_E.png" width="256" height="256"/>
 </tile>
 <tile id="12">
  <image source="../kenney-sketch-town/grass_waterConvex_S.png" width="256" height="256"/>
 </tile>
 <tile id="13">
  <image source="../kenney-sketch-town/grass_waterConvex_W.png" width="256" height="256"/>
 </tile>
 <wangsets>
  <wangset name="terrain" type="corner" tile="1">
   <wangcolor name="grass" color="#ff0000" tile="-1" probability="1"/>
   <wangcolor name="water" color="#00ff00" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="1" wangid="0,2,0,2,0,2,0,2"/>
   <wangtile tileid="2" wangid="0,1,0,2,0,2,0,1"/>
   <wangtile tileid="3" wangid="0,1,0,1,0,2,0,2"/>
   <wangtile tileid="4" wangid="0,2,0,1,0,1,0,2"/>
   <wangtile tileid="5" wangid="0,2,0,2,0,1,0,1"/>
   <wangtile tileid="6" wangid="0,1,0,2,0,1,0,1"/>
   <wangtile tileid="7" wangid="0,1,0,1,0,2,0,1"/>
   <wangtile tileid="8" wangid="0,1,0,1,0,1,0,2"/>
   <wangtile tileid="9" wangid="0,2,0,1,0,1,0,1"/>
   <wangtile tileid="10" wangid="0,2,0,2,0,2,0,1"/>
   <wangtile tileid="11" wangid="0,1,0,2,0,2,0,2"/>
   <wangtile tileid="12" wangid="0,2,0,1,0,2,0,2"/>
   <wangtile tileid="13" wangid="0,2,0,2,0,1,0,2"/>
  </wangset>
 </wangsets>
</tileset>
