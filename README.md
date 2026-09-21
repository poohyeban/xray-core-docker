# Xray Docker stack

三个服务：官方 Xray、用于固定配置校验与重启的 Rust controller、负责 geodata 更新和日志轮转的 updater。适用于 Linux amd64 VPS，Docker Engine 与 Docker Compose 需提前安装。

仓库只保存部署源码。生产配置、客户端身份、证书、日志、geodata 及备份均不进入仓库或镜像。这里不包含任何现用服务器的节点、中转和分流设置。

## 在新 VPS 部署

以 root 操作，在尚未运行这套服务的新目录执行：

```sh
git clone https://github.com/poohyeban/xray-core-docker.git
cd xray-core-docker
cp .env.example .env
python3 scripts/prepare.py
# 自行创建 xray/config/config.json；若配置使用证书，也自行放入 xray/certs/。
# 不要上传自己的配置和证书。
chmod 600 xray/config/config.json
# 按需要配置 VPS 防火墙，放行你选择的入站端口（默认 TCP 443）。
docker compose up -d --wait
docker compose ps
```

准备脚本仅拉取镜像、创建运行目录、下载并校验初始 geodata。**不会生成 config.json、UUID、密钥或任何默认代理配置。** 缺少 `xray/config/config.json` 或配置无效时，Xray 无法正常启动；这是要求使用者自行配置的预期行为。

请自行按官方 Xray 配置格式创建文件。配置中的日志路径可使用 `/var/log/xray/access.log` 和 `/var/log/xray/error.log`；证书目录是 `/etc/xray/certs`，geodata 目录是 `/usr/local/share/xray`。Xray 使用宿主网络，因此监听端口和网络行为完全由你的配置决定。

再次运行准备脚本不会更改已有配置或覆盖完整的 geodata。若下载中断导致仅存在一个 geodata 文件，脚本会停止，请先检查后再处理。脚本不会修改防火墙、SSH 或宿主网络配置。

WARP 和中转节点不是这三个容器自动提供的服务。如果你的配置引用这些出口，必须另行部署对应服务。生产配置始终只保存在本机，不通过 GitHub 分发。

## 镜像与更新

- 核心使用 `ghcr.io/xtls/xray-core:26.3.27`，Compose 同时固定了镜像摘要。
- controller：`ghcr.io/poohyeban/xray-core-docker/controller:latest`
- updater：`ghcr.io/poohyeban/xray-core-docker/geodata-updater:latest`

两个辅助镜像由本仓库构建，通过集成检查后发布 `latest` 与 `sha-<完整提交SHA>`。可在本机 `.env` 中用 `CONTROLLER_IMAGE`、`UPDATER_IMAGE` 固定提交标签，便于回滚。修改 `XRAY_IMAGE` 可单独升级核心，升级后应重新检查配置兼容性。

```sh
git pull --ff-only
docker compose pull
docker compose up -d --wait
```

该过程保留绑定挂载的配置和日志；不要盲目删除 `xray/`。如需迁移现有身份，使用私密的服务器间传输，不能通过公开仓库中转。

## 运行方式

- Xray 使用宿主网络，只读根文件系统，并显式限制 capabilities；开放的端口由本机 Xray 配置决定。
- controller 无网络接口，通过 Unix socket 提供健康检查及无参数的 `/apply`；依据 Compose 项目标签定位唯一 Xray，不依赖固定容器名。
- 只有 controller 挂载 Docker socket。`:ro` 挂载不等于 Docker API 只读：controller 仍具有调用 Docker API 的权限，需将其视为受信任的管理组件。
- updater 不挂载 Docker socket，通过 controller 完成固定配置校验和重启。
- geodata 默认每日 04:15 更新（Asia/Taipei）；校验 SHA256，失败时尝试回滚。可改 `.env` 的 `TZ`；修改计划需调整 `root.cron` 并重建 updater。
- 日志轮转每天 03:30 检查。Xray 日志保留 30 份，updater 保留 8 份。文件大小限制在定时检查时执行，不是实时磁盘硬限额；Docker 自身日志限制为每个容器 3 × 10 MB。

## 检查与故障定位

```sh
docker compose ps
docker compose exec -T xray /usr/local/bin/xray run -test -c /usr/local/etc/xray/config.json
docker compose logs --tail 50 xray-controller geodata-updater
```

健康检查和 CI 覆盖配置加载、容器启动、controller 重启与 geodata 更新流程，不代替真实客户端的 REALITY 握手和出口验收。不要将原始访问日志贴到公开 issue。

源码开发时可直接从 `xray-controller` 和 `xray-geodata-updater` 目录构建镜像，再使用 `.env` 指向本地标签。`prepare.py --skip-pull` 仅用于已经准备好所有镜像的场景。

## 发布隐私边界

- `.gitignore` 排除所有运行目录；两个 `.dockerignore` 对构建上下文使用文件白名单。
- CI 首先检查追踪文件白名单及常见凭据，并在测试 runner 上临时使用不包含凭据、不监听入站端口的临时测试配置。
- 构建使用 Docker CLI，关闭 provenance，不上传构建记录和运行配置附件。
- 提交使用公开账户别名及 GitHub noreply 邮箱；推送事件中的邮箱在其他步骤前加入日志遮罩。
- 自动扫描不能保证识别任意秘密；发布前仍应审阅 Git 历史、日志与镜像。不要通过修改扫描白名单来加入生产配置。

官方参考：[Docker Compose](https://docs.docker.com/compose/)、[Xray](https://github.com/XTLS/Xray-core)、[geodata 来源](https://github.com/Loyalsoldier/v2ray-rules-dat)。
