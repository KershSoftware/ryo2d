<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.11" tiledversion="1.12.2" name="sketch-town-raised" tilewidth="256" tileheight="256" tilecount="4" columns="0">
 <tileoffset x="-12" y="6"/>
 <grid orientation="orthogonal" width="0" height="0"/>
 <tile id="0">
  <image source="../kenney-sketch-town/grass_center_N.png" width="256" height="256"/>
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0">
    <polygon points="128,140 244,195 128,250 12,195"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="1">
  <properties>
   <property name="stairs" type="int" value="1"/>
  </properties>
  <image source="../kenney-sketch-town/grass_pathSlope_E.png" width="256" height="256"/>
 </tile>
 <tile id="2">
  <image source="../kenney-sketch-town/rocks_grass_N.png" width="256" height="256"/>
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0">
    <polygon points="128,140 244,195 128,250 12,195"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="3">
  <properties>
   <property name="sort_offset" type="float" value="55"/>
  </properties>
  <image source="../kenney-sketch-town/tree_single_N.png" width="256" height="256"/>
 </tile>
</tileset>
