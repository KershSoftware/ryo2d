<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="tiles" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="../../images/tiles.png" width="64" height="64"/>
 <tile id="1" type="brick" probability="0.5">
  <properties>
   <property name="hardness" type="int" value="3"/>
   <property name="label" value="a brick"/>
  </properties>
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="8" width="16" height="8">
    <properties>
     <property name="one_way" type="bool" value="true"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
 <tile id="2">
  <objectgroup draworder="index" id="2">
   <object id="1" x="2" y="2">
    <polygon points="0,0 12,0 6,12"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="3">
  <objectgroup draworder="index" id="2">
   <object id="1" x="0" y="0" width="16" height="16">
    <properties>
     <property name="occluder" type="bool" value="true"/>
    </properties>
    <ellipse/>
   </object>
  </objectgroup>
 </tile>
 <tile id="4">
  <animation>
   <frame tileid="4" duration="100"/>
   <frame tileid="5" duration="250"/>
  </animation>
 </tile>
</tileset>
