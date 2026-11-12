pub fn rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let h=h.rem_euclid(360.)/60.;let s=s.clamp(0.,1.);let v=v.clamp(0.,1.);
    let c=v*s;let x=c*(1.-(h%2.-1.).abs());let m=v-c;
    let [r,g,b]=match h as u8 {0=>[c,x,0.],1=>[x,c,0.],2=>[0.,c,x],3=>[0.,x,c],4=>[x,0.,c],_=>[c,0.,x]};
    [r,g,b].map(|value|((value+m)*255.).round() as u8)
}
pub fn hsv([r,g,b]:[u8;3])->[f64;3]{
    let [r,g,b]=[r,g,b].map(|v|v as f64/255.);let max=r.max(g).max(b);let min=r.min(g).min(b);let d=max-min;
    let h=if d==0.{0.}else if max==r{60.*((g-b)/d).rem_euclid(6.)}else if max==g{60.*((b-r)/d+2.)}else{60.*((r-g)/d+4.)};
    [h,if max==0.{0.}else{d/max},max]
}
pub fn parse(hex:&str)->Option<[u8;3]>{
    let text=hex.strip_prefix('#')?;if text.len()!=6||!text.is_ascii(){return None;}
    Some([u8::from_str_radix(&text[0..2],16).ok()?,u8::from_str_radix(&text[2..4],16).ok()?,u8::from_str_radix(&text[4..6],16).ok()?])
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn hsv_roundtrips_colors_and_bounds(){for r in (0..=255).step_by(17){for g in (0..=255).step_by(17){for b in (0..=255).step_by(17){let original=[r,g,b];let [h,s,v]=hsv(original);assert_eq!(rgb(h,s,v),original);}}}assert_eq!(rgb(360.,1.,1.),[255,0,0]);assert_eq!(rgb(0.,-1.,2.),[255,255,255]);}
    #[test]fn hexadecimal_rejects_partial_and_non_ascii(){assert_eq!(parse("#1aB2c3"),Some([26,178,195]));for bad in ["#FFF","123456","#00000x","#颜色00"]{assert!(parse(bad).is_none());}}
}
