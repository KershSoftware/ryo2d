<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="collision" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="../../images/tiles.png" width="64" height="64"/>
 <tile id="0">
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0" width="16" height="16"/>
  </objectgroup>
 </tile>
 <tile id="1">
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0">
    <polygon points="16,0 16,16 0,16"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="2">
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0" width="16" height="16">
    <properties>
     <property name="one_way" type="bool" value="true"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
 <tile id="3">
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="8" width="16" height="8"/>
  </objectgroup>
 </tile>
 <tile id="4">
  <objectgroup draworder="index" id="2">
   <object id="1" x="4" y="2" width="8" height="12">
    <properties>
     <property name="occluder" type="bool" value="true"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
</tileset>
