#version 330 core
in vec2 uv;
out vec4 pixel;
uniform sampler2D source;
uniform vec4 crop;
uniform vec2 source_size;
uniform vec2 surface_size;
uniform int quality;
uniform float opacity;
void main() {
    vec2 local = vec2(uv.x, 1.0-uv.y);
    vec2 p = (local-0.5)*surface_size;
    float radius = 24.0;
    vec2 q = abs(p)-(surface_size*0.5-radius);
    float distance = length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;
    float alpha = 1.0-smoothstep(-1.0,1.0,distance);
    if (alpha==0.0) { pixel=vec4(0.0); return; }
    vec2 sample_uv = crop.xy+local*crop.zw;
    float edge = exp(-max(-distance,0.0)/12.0);
    if (quality==2) {
        vec2 normal = normalize(p+vec2(0.001));
        sample_uv += normal*edge*7.0/source_size;
    }
    vec3 color = texture(source,sample_uv).rgb;
    if (quality>0) {
        vec2 step_uv = vec2(quality==2?4.0:2.0)/source_size;
        color *= 4.0;
        color += 2.0*(texture(source,sample_uv+vec2(step_uv.x,0)).rgb+texture(source,sample_uv-vec2(step_uv.x,0)).rgb);
        color += 2.0*(texture(source,sample_uv+vec2(0,step_uv.y)).rgb+texture(source,sample_uv-vec2(0,step_uv.y)).rgb);
        color += texture(source,sample_uv+step_uv).rgb+texture(source,sample_uv-step_uv).rgb;
        color += texture(source,sample_uv+vec2(step_uv.x,-step_uv.y)).rgb+texture(source,sample_uv+vec2(-step_uv.x,step_uv.y)).rgb;
        color /= 16.0;
    }
    color=mix(color,vec3(0.92,0.95,1.0),0.22+0.56*opacity);
    if (quality==2) {
        float light = 0.5+0.5*dot(normalize(p+vec2(0.001)),normalize(vec2(-0.6,-0.8)));
        color += vec3(edge*light*0.22);
    }
    float border = 1.0-smoothstep(0.0,1.8,abs(distance+1.0));
    color=mix(color,vec3(1.0),border*(quality==2?0.8:0.4));
    pixel=vec4(clamp(color,0.0,1.0)*alpha,alpha);
}
