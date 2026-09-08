import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description="Regenerate browser statistics fixtures with the pinned GNU R oracle")
parser.add_argument("oracle", help="Path to the pinned oracle Rscript binary")
oracle = parser.parse_args().oracle
revision = subprocess.check_output([oracle, "--vanilla", "-e", 'cat(R.version[["svn rev"]])'], text=True).strip()
if revision != "90451":
    raise SystemExit(f"Expected pinned oracle SVN revision 90451, got {revision}")
cases=[
('fft real','z <- fft(c(1,2,3,4)); cat(c(Re(z), Im(z)), sep=",")'),
('fft complex prime','x <- complex(real=1:7, imaginary=7:1); z <- fft(x); cat(c(Re(z), Im(z)), sep=",")'),
('fft inverse','z <- fft(fft(c(1,2,3,4)), inverse=TRUE); cat(c(Re(z), Im(z)), sep=",")'),
('fft array','z <- fft(array(1:12, dim=c(2,3,2))); cat(c(dim(z), Re(z), Im(z)), sep=",")'),
('mvfft columns','z <- mvfft(matrix(1:12, nrow=4)); cat(c(dim(z), Re(z), Im(z)), sep=",")'),
('mvfft inverse','x <- matrix(1:12,nrow=4); z <- mvfft(mvfft(x), inverse=TRUE); cat(c(dim(z), Re(z), Im(z)), sep=",")'),
]
functions={'rpois':'lambda=3','rexp':'rate=2','rchisq':'df=4','rgeom':'prob=.3','rt':'df=5','rsignrank':'n=6','rbeta':'shape1=2,shape2=3','rbinom':'size=10,prob=.4','rcauchy':'location=1,scale=2','rf':'df1=4,df2=6','rgamma':'shape=2,rate=3','rlnorm':'meanlog=.2,sdlog=.7','rlogis':'location=1,scale=2','rnbinom':'size=3,prob=.6','rweibull':'shape=2,scale=3','rwilcox':'m=4,n=5','rhyper':'m=7,n=9,k=5'}
for fn,args in functions.items():
 # Some R functions use n as both sample size and distribution parameter; positional first arg avoids duplicate names.
 code=f'set.seed(42); x <- {fn}(8, {args}); cat(c(x, runif(3)), sep=",")'
 cases.append((fn,code))
cases.extend([
    ('rexp recycling', 'set.seed(42); cat(c(rexp(6,rate=c(1,2)),runif(3)),sep=",")'),
    ('rbinom degenerate', 'set.seed(42); cat(c(rbinom(4,size=0,prob=.4),runif(3)),sep=",")'),
    ('rnbinom mean', 'set.seed(42); cat(c(rnbinom(8,size=3,mu=2),runif(3)),sep=",")'),
    ('rgamma scale', 'set.seed(42); cat(c(rgamma(8,shape=2,scale=3),runif(3)),sep=",")'),
    ('rexp vector n', 'set.seed(42); cat(c(rexp(c(1,2,3),rate=2),runif(3)),sep=",")'),
    ('rpois empty', 'set.seed(42); cat(c(rpois(0,lambda=3),runif(3)),sep=",")'),
])
output=[]
for name,code in cases:
 r=subprocess.run([oracle,'--vanilla','-e',code],capture_output=True,text=True,check=True)
 output.append({'name':name,'code':code,'expected':[float(v) for v in r.stdout.strip().split(',')]})
with (Path(__file__).resolve().parents[1] / 'tests/fixtures/wasm-stats-oracle.json').open('w') as f: json.dump({'oracleCommit':'bac583951b728e97b9786804d3b4081f0fe18df5','cases':output},f,indent=2);f.write('\n')
