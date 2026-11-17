# Convertit la sortie de `docker stats` (vue cgroup du conteneur) en CSV.
# Entrée : ts;CPU%;mem / limite;net rx / tx;blk r / w;pids
# Sortie : ts,cpu_pct,mem_mb,mem_limit_mb,net_rx_mb,net_tx_mb,blk_read_mb,blk_write_mb,pids

BEGIN { FS = ";"; OFS = "," }

function mb(s,   n, u) {
  gsub(/ /, "", s)
  n = s + 0
  u = s
  sub(/^[0-9.eE+-]+/, "", u)
  if (u == "B") return n / 1048576
  if (u == "kB" || u == "KB") return n * 1000 / 1048576
  if (u == "MB") return n * 1000000 / 1048576
  if (u == "GB") return n * 1000000000 / 1048576
  if (u == "KiB") return n / 1024
  if (u == "MiB") return n
  if (u == "GiB") return n * 1024
  if (u == "TiB") return n * 1048576
  return n
}

{
  cpu = $2; sub(/%/, "", cpu)
  split($3, mem, " / ")
  split($4, net, " / ")
  split($5, blk, " / ")
  printf "%s,%.1f,%.1f,%.1f,%.2f,%.2f,%.2f,%.2f,%d\n", $1, cpu, mb(mem[1]), mb(mem[2]), mb(net[1]), mb(net[2]), mb(blk[1]), mb(blk[2]), $6
  fflush()
}
