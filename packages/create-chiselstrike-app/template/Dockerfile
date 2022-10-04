FROM frolvlad/alpine-glibc:alpine-3.13

COPY . /opt/app

WORKDIR /opt/app

RUN apk add npm

RUN npm i

EXPOSE 8080/tcp

CMD npm run dev -- -- --api-listen-addr 0.0.0.0:8080
