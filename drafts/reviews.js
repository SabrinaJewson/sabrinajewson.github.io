"use strict";const t=[...document.getElementById("rows").content.children],e=16,o=document.getElementById("reviews"),c=[];for(const t of document.getElementsByClassName("page-controls")){const e=document.createElement("button"),o=document.createElement("button"),n=document.createElement("button"),r=document.createElement("button"),s=document.createElement("button");e.append("0"),n.append("<"),r.append(">"),e.classList.add("pretty"),o.classList.add("pretty"),n.classList.add("pretty"),r.classList.add("pretty"),s.classList.add("pretty"),t.append(e,n,s,r,o),c.push({t:e,o:o,i:n,next:r,l:s})}const n=document.getElementById("score-header"),[r,s]=document.getElementById("filter").children,i="↕",l="↑",a="↓",u="Score",d="Date",f=[{u:u,direction:i},{u:u,direction:a},{u:u,direction:l},{u:d,direction:a},{u:d,direction:l}];let m,b,p,y;function k(o,s){let y;if(m=o,b=s,r.value=s,history.replaceState(null,"",location.origin+location.pathname+(""===s?"":`?q=${s}`)),null!==s){const e=s.toLowerCase().split(" ");y=t.filter((t=>{const o=t.content.firstElementChild.textContent.toLowerCase();return e.every((t=>o.includes(t)))}))}else y=[...t];const k=f[o];let w,v;switch(k.direction){case i:w=0;break;case l:w=1;break;case a:w=-1}switch(k.u){case d:v=t=>{const e=t.content.firstElementChild.getElementsByTagName("time")[0];return(e&&w*Date.parse(e.dateTime))??1/0};break;case u:v=t=>{const e=t.content.firstElementChild.getElementsByClassName("score")[0];return(e&&w*parseFloat(e.textContent))??1/0}}0!==w&&y.sort(((t,e)=>v(t)-v(e))),n.textContent=`${k.u} ${k.direction}`,p=[];for(let t=0;t<y.length;t+=e)p.push(y.slice(t,t+e));0===p.length&&p.push([]);for(const{o:t}of c)t.replaceChildren(p.length-1);h(0)}function h(t){o.replaceChildren();for(const e of p[t])for(const t of e.content.children)o.append(t.cloneNode(!0));for(const{t:e,o:o,i:n,next:r,l:s}of c)0===t?(e.disabled=!0,n.disabled=!0):0===y&&(e.disabled=!1,n.disabled=!1),t===p.length-1?(o.disabled=!0,r.disabled=!0):(o.disabled=!1,r.disabled=!1),s.replaceChildren(t);y=t}n.parentElement.addEventListener("click",(()=>{k((m+1)%f.length,b)}));for(const{t:t,o:e,i:o,next:n,l:r}of c){t.addEventListener("click",(()=>h(0))),e.addEventListener("click",(()=>h(p.length-1))),o.addEventListener("click",(()=>h(Math.max(0,y-1)))),n.addEventListener("click",(()=>h(Math.min(p.length-1,y+1))));let c=!1;r.addEventListener("click",(()=>{if(c)return;c=!0;const t=document.createElement("input");r.replaceChildren(t);const e=()=>{if(!c)return;c=!1;let e=parseInt(t.value);isNaN(e)?r.replaceChildren(y):(e=Math.max(0,Math.min(p.length-1,e)),e!==y?h(e):r.replaceChildren(y))};t.addEventListener("blur",(()=>e())),t.addEventListener("keypress",(t=>{"Enter"===t.key&&e()})),t.focus()}))}r.addEventListener("input",(()=>k(m,r.value))),s.addEventListener("click",(()=>k(m,""))),k(0,new URLSearchParams(location.search).get("q")||r.value);
