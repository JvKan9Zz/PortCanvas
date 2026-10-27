export async function checkStartupUpdate(info,state,check){
 if(info?.updatesSupported!==true||['checking','downloading','verifying','installing','ready'].includes(state?.status))return null;
 try{return await check();}catch{return null;}
}
export function hasAvailableUpdate(state){
 return Boolean(state?.version)&&['available','downloading','verifying','ready','canceled','error'].includes(state.status);
}
