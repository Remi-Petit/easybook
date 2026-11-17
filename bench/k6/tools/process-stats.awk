# Agrège les lignes de remote-sampler.sh par process (easybook, node, nginx).
# CPU en % d'un cœur (200 = 2 cœurs saturés), mémoire RSS en Mo.
# Sortie : ts,process,cpu_pct,rss_mb,threads,fds,procs

BEGIN {
  FS = ","
  want["easybook"] = 1
  want["node"] = 1
  want["nginx"] = 1
}

function flush(   c) {
  for (c in seen) {
    printf "%s,%s,%.1f,%.1f,%d,%d,%d\n", cur, c, cpu[c], rss[c], thr[c], fds[c], n[c]
  }
  fflush()
  delete seen; delete cpu; delete rss; delete thr; delete fds; delete n
}

{
  comm = $3
  if (!(comm in want)) next
  if (cur != "" && $1 != cur) flush()
  cur = $1

  # 100 ticks/s : d ticks sur dt secondes = d/dt % d'un cœur.
  if (($2 in prevTs) && $1 + 0 > prevTs[$2]) {
    d = $4 - prevTicks[$2]
    if (d < 0) d = 0
    cpu[comm] += d / ($1 - prevTs[$2])
  }
  prevTs[$2] = $1 + 0
  prevTicks[$2] = $4

  rss[comm] += $5 * 4 / 1024   # pages de 4 Kio
  thr[comm] += $6
  if ($7 + 0 > fds[comm]) fds[comm] = $7 + 0
  n[comm]++
  seen[comm] = 1
}

END { flush() }
