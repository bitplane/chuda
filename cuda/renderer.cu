#include <cuda_runtime.h>
#include <cstdint>
#include <cstdio>

struct Choice { uint32_t codepoint; uint8_t fg[3], bg[3], transparent_bg; };
struct Candidate { uint64_t error; uint32_t index; uint8_t fg[3], bg[3], transparent_bg; };

__global__ void score(const uint8_t *pixels, uint32_t cells, const uint64_t *masks,
                      const uint32_t *codes, uint32_t nsymbols, uint64_t edge_bias, Choice *out) {
    unsigned cell = blockIdx.x, tid = threadIdx.x;
    if (cell >= cells) return;
    extern __shared__ Candidate candidates[];
    Candidate me = {UINT64_MAX, tid, {}, {}, 0};
    const uint8_t *p = pixels + size_t(cell) * 256;
    for (unsigned symbol = tid; symbol < nsymbols; symbol += blockDim.x) {
        Candidate opaque = {0, symbol, {}, {}, 0};
        Candidate edge = {0, symbol, {}, {}, 1};
        uint32_t weight[2] = {};
        uint32_t sum[2][3] = {}, sum_squares[2][3] = {};
        uint32_t alpha_error_opaque = 0, alpha_error_edge = 0;
        uint64_t mask = masks[symbol];
        #pragma unroll
        for (int i=0; i<64; i++) {
            int side=(mask>>(63-i))&1;
            uint32_t alpha=p[i*4+3];
            weight[side]+=alpha;
            int opaque_delta=255-int(alpha);
            int edge_delta=(side ? 255 : 0)-int(alpha);
            alpha_error_opaque+=opaque_delta*opaque_delta;
            alpha_error_edge+=edge_delta*edge_delta;
            #pragma unroll
            for(int c=0;c<3;c++) {
                uint32_t value=p[i*4+c];
                sum[side][c]+=alpha*value;
                sum_squares[side][c]+=alpha*value*value;
            }
        }
        uint32_t total_weight=weight[0]+weight[1];
        #pragma unroll
        for (int side=0;side<2;side++) {
            #pragma unroll
            for(int c=0;c<3;c++) {
                uint32_t mean = weight[side] ? sum[side][c]/weight[side]
                    : (total_weight ? (sum[0][c]+sum[1][c])/total_weight : 0);
                (side?opaque.fg:opaque.bg)[c] = mean;
                uint64_t rgb_error = uint64_t(sum_squares[side][c])
                    - 2ull*mean*sum[side][c] + uint64_t(weight[side])*mean*mean;
                opaque.error += rgb_error;
                if (side) {
                    edge.fg[c]=mean;
                    edge.error += rgb_error;
                }
            }
        }
        // RGB error is alpha-weighted (0..255), so scale alpha SSE into the
        // same units. Three makes opacity as important as all RGB channels.
        opaque.error += 3ull*255*alpha_error_opaque;
        edge.error += 3ull*255*alpha_error_edge + edge_bias;
        Candidate current = edge.error < opaque.error ? edge : opaque;
        if (current.error < me.error || (current.error == me.error && current.index < me.index)) me = current;
    }
    candidates[tid]=me; __syncthreads();
    for(unsigned stride=blockDim.x/2;stride;stride>>=1) { if(tid<stride) { Candidate a=candidates[tid],b=candidates[tid+stride]; if(b.error<a.error || (b.error==a.error && b.index<a.index)) candidates[tid]=b; } __syncthreads(); }
    if(tid==0) { Candidate b=candidates[0]; out[cell].codepoint=codes[b.index]; out[cell].transparent_bg=b.transparent_bg; for(int c=0;c<3;c++){out[cell].fg[c]=b.fg[c];out[cell].bg[c]=b.bg[c];} }
}
static int failure(cudaError_t e, char *message, size_t capacity) { if(message&&capacity) snprintf(message,capacity,"%s",cudaGetErrorString(e)); return int(e); }

static uint8_t *cached_pixels = nullptr;
static uint64_t *cached_masks = nullptr;
static uint32_t *cached_codes = nullptr;
static Choice *cached_output = nullptr;
static uint32_t cached_cells = 0;
static uint32_t cached_symbols = 0;

extern "C" int cb_render_cuda(const uint8_t *pixels,uint32_t cells,const uint64_t *masks,const uint32_t *codes,
 uint32_t nsymbols,Choice *output,float transparent_threshold,char *message,size_t capacity) {
    cudaError_t e=cudaSuccess;
    uint64_t edge_bias=uint64_t(transparent_threshold*64.0f*65025.0f*255.0f*3.0f);
    #define CU(x) do { if((e=(x))!=cudaSuccess){failure(e,message,capacity);goto done;} } while(0)
    if (cells > cached_cells) {
        cudaFree(cached_pixels); cached_pixels=nullptr;
        cudaFree(cached_output); cached_output=nullptr;
        CU(cudaMalloc(&cached_pixels,size_t(cells)*256));
        CU(cudaMalloc(&cached_output,size_t(cells)*sizeof(Choice)));
        cached_cells=cells;
    }
    if (nsymbols != cached_symbols) {
        cudaFree(cached_masks); cached_masks=nullptr;
        cudaFree(cached_codes); cached_codes=nullptr;
        CU(cudaMalloc(&cached_masks,size_t(nsymbols)*8));
        CU(cudaMalloc(&cached_codes,size_t(nsymbols)*4));
        CU(cudaMemcpy(cached_masks,masks,size_t(nsymbols)*8,cudaMemcpyHostToDevice));
        CU(cudaMemcpy(cached_codes,codes,size_t(nsymbols)*4,cudaMemcpyHostToDevice));
        cached_symbols=nsymbols;
    }
    CU(cudaMemcpy(cached_pixels,pixels,size_t(cells)*256,cudaMemcpyHostToDevice));
    score<<<cells,128,128*sizeof(Candidate)>>>(cached_pixels,cells,cached_masks,cached_codes,nsymbols,edge_bias,cached_output);
    CU(cudaGetLastError());
    CU(cudaMemcpy(output,cached_output,size_t(cells)*sizeof(Choice),cudaMemcpyDeviceToHost));
done: return int(e);
}
