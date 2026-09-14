"""The single model-controller loop shared by standalone and P4 transports."""
import torch


def drive(pipeline, requests, schedule, tokenizer, reference=None, cache_checker=None):
    pending=list(requests)
    completed=[]
    while pending:
        current=pending.pop(0)
        prefill=current.position<len(current.tokens)
        ids=current.tokens[current.position:current.position+current.chunk] if prefill else [current.generated[-1]]
        tokens=torch.tensor([ids],dtype=torch.int64)
        logits=pipeline.step(current.session_id,current.issue,current.position,tokens)
        if reference:
            reference.compare(current.session_id,tokens,logits)
        if cache_checker:
            cache_checker(current.session_id,reference.caches[current.session_id])
        current.position+=len(ids)
        current.issue+=1
        if current.position>=len(current.tokens):
            token=int(logits.argmax(-1).item())
            current.generated.append(token)
            if token==tokenizer.eos_token_id:
                current.terminal="eos"
            elif current.cancel_after is not None and len(current.generated)>=current.cancel_after:
                current.terminal="cancelled_at_step_boundary"
            elif len(current.generated)>=current.limit:
                current.terminal="length"
        if current.terminal:
            # The standalone transport retains its original release API.
            if hasattr(pipeline,"cancel") and current.terminal=="cancelled_at_step_boundary":
                pipeline.cancel(current.session_id)
            else:
                pipeline.release(current.session_id)
            if reference:
                reference.release(current.session_id)
            completed.append({"id":current.name,"input_tokens":len(current.tokens),"generated_tokens":current.generated,
                "terminal":current.terminal,"text":tokenizer.decode(current.generated,skip_special_tokens=True),"released":True})
        elif schedule=="round_robin":
            pending.append(current)
        else:
            pending.insert(0,current)
    return completed
