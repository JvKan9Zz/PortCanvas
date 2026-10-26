const clamp=(value,min,max)=>Math.max(min,Math.min(max,value));
export function initialCrop(imageAspect,targetAspect){
 const ratio=targetAspect/imageAspect,width=Math.min(1,ratio),height=width/ratio;
 return {x:(1-width)/2,y:(1-height)/2,width,height};
}
export function moveCrop(crop,dx,dy){return {...crop,x:clamp(crop.x+dx,0,1-crop.width),y:clamp(crop.y+dy,0,1-crop.height)};}
export function resizeCrop(crop,corner,dx,dy,ratio){
 const west=corner.includes('w'),north=corner.includes('n'),anchorX=west?crop.x+crop.width:crop.x,anchorY=north?crop.y+crop.height:crop.y;
 const maximum=Math.min(west?anchorX:1-anchorX,(north?anchorY:1-anchorY)*ratio);
 const delta=Math.abs(dx)>=Math.abs(dy*ratio)?dx*(west?-1:1):dy*ratio*(north?-1:1);
 const width=clamp(crop.width+delta,Math.min(.03,maximum),maximum),height=width/ratio;
 return {x:west?anchorX-width:anchorX,y:north?anchorY-height:anchorY,width,height};
}
export function projectCrop(imageWidth,imageHeight,crop,viewportWidth,viewportHeight){
 const scale=Math.max(viewportWidth/(imageWidth*crop.width),viewportHeight/(imageHeight*crop.height));
 const width=imageWidth*scale,height=imageHeight*scale;
 return {width,height,x:(viewportWidth-crop.width*width)/2-crop.x*width,y:(viewportHeight-crop.height*height)/2-crop.y*height};
}
