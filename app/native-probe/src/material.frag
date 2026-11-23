#version 330 core
in vec2 uv;
out vec4 pixel;
uniform sampler2D source;
uniform vec4 crop;
uniform vec2 source_size;
uniform vec2 surface_size;
uniform float corner_radius;
uniform vec3 tint;
uniform int quality;
uniform float opacity;
void main() {
    vec2 local=vec2(uv.x,1.0-uv.y);
    vec2 p=(local-0.5)*surface_size;
    float radius=min(corner_radius,min(surface_size.x,surface_size.y)*0.5);
    vec2 q=abs(p)-(surface_size*0.5-radius);
    vec2 corner=max(q,0.0);
    float distance=pow(pow(corner.x,4.0)+pow(corner.y,4.0),0.25)+min(max(q.x,q.y),0.0)-radius;
    float alpha=1.0-smoothstep(-1.0,1.0,distance);
    if(alpha==0.0){pixel=vec4(0.0);return;}
    float amount=clamp(opacity,0.0,1.0);
    if(amount==1.0){pixel=vec4(tint*alpha,alpha);return;}
    float edge=1.0-smoothstep(0.0,20.0,max(-distance,0.0));
    vec2 sample_uv=crop.xy+local*crop.zw;
    // Use the rounded surface normal, including on the straight edges.
    vec2 normal=sign(p)*(q.x>q.y?vec2(1.0,0.0):vec2(0.0,1.0));
    if(corner.x>0.0&&corner.y>0.0){normal=sign(p)*normalize(corner*corner*corner);}
    vec2 logical_uv=crop.zw/surface_size;
    if(quality==2){sample_uv-=normal*edge*14.0*logical_uv;}
    vec3 color=texture(source,sample_uv).rgb;
    if(quality>0){
        vec2 d=quality==2?logical_uv*(3.0+5.0*edge):vec2(3.0)/source_size;
        color*=4.0;
        color+=2.0*(texture(source,sample_uv+vec2(d.x,0)).rgb+texture(source,sample_uv-vec2(d.x,0)).rgb);
        color+=2.0*(texture(source,sample_uv+vec2(0,d.y)).rgb+texture(source,sample_uv-vec2(0,d.y)).rgb);
        color+=texture(source,sample_uv+d).rgb+texture(source,sample_uv-d).rgb;
        color+=texture(source,sample_uv+vec2(d.x,-d.y)).rgb+texture(source,sample_uv+vec2(-d.x,d.y)).rgb;
        color/=16.0;
    }
    // Keep native foreground legible even at the most transparent preference.
    float dark=1.0-step(0.5,dot(tint,vec3(0.2126,0.7152,0.0722)));
    color=mix(color,tint,mix(0.22+dark*0.02,1.0,amount));
    if(quality==2){
        float light=0.5+0.5*dot(normal,vec2(-0.6,-0.8));
        color+=vec3(edge*light*(1.0-amount)*mix(0.14,0.065,dark));
    }
    pixel=vec4(clamp(color,0.0,1.0)*alpha,alpha);
}
