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
    float edge=exp(-max(-distance,0.0)/10.0);
    vec2 sample_uv=crop.xy+local*crop.zw;
    if(quality==2){sample_uv+=normalize(p+vec2(0.001))*edge*5.0/source_size;}
    vec3 color=texture(source,sample_uv).rgb;
    if(quality>0){
        vec2 d=vec2(quality==2?5.0:3.0)/source_size;
        color*=4.0;
        color+=2.0*(texture(source,sample_uv+vec2(d.x,0)).rgb+texture(source,sample_uv-vec2(d.x,0)).rgb);
        color+=2.0*(texture(source,sample_uv+vec2(0,d.y)).rgb+texture(source,sample_uv-vec2(0,d.y)).rgb);
        color+=texture(source,sample_uv+d).rgb+texture(source,sample_uv-d).rgb;
        color+=texture(source,sample_uv+vec2(d.x,-d.y)).rgb+texture(source,sample_uv+vec2(-d.x,d.y)).rgb;
        color/=16.0;
    }
    // Keep native foreground legible even at the most transparent preference.
    color=mix(color,tint,0.22+0.58*opacity+(1.0-step(0.5,dot(tint,vec3(0.2126,0.7152,0.0722))))*0.02);
    float dark=1.0-step(0.5,dot(tint,vec3(0.2126,0.7152,0.0722)));
    if(quality==2){
        float light=0.5+0.5*dot(normalize(p+vec2(0.001)),normalize(vec2(-0.6,-0.8)));
        color+=vec3(edge*light*mix(0.12,0.035,dark));
    }
    pixel=vec4(clamp(color,0.0,1.0)*alpha,alpha);
}
