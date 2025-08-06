package co.juandevgo.paleoapi.application.mapper;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.domain.Specie;
import co.juandevgo.paleoapi.infrastructure.entity.SpecieEntity;
import org.mapstruct.Mapper;

import java.util.ArrayList;
import java.util.List;

@Mapper(componentModel = "spring")
public interface SpeciesMapper {

    SpecieEntity toEntity(SpecieDTO dto);

    SpecieEntity toEntity(Specie specie);

    SpecieDTO toDTO(SpecieEntity entity);

    SpecieDTO toDTO(Specie specie);

    Specie toDomainEntity(SpecieDTO dto);

    Specie toDomainEntity(SpecieEntity entity);

    default List<SpecieDTO> toDTOFromEntityLists(List<SpecieEntity> entityList) {
        List<SpecieDTO> DTOList = new ArrayList<>();
        if(entityList != null) {
            DTOList.addAll(entityList.stream().map(this::toDTO).toList());
        }
        return DTOList;
    }

    default List<SpecieDTO> toDTOFromDomainEntityLists(List<Specie> domainEntityList) {
        List<SpecieDTO> DTOList = new ArrayList<>();
        if(domainEntityList != null) {
            DTOList.addAll(domainEntityList.stream().map(this::toDTO).toList());
        }
        return DTOList;
    }

    default List<SpecieEntity> toEntityFromDTOLists(List<SpecieDTO> DTOList) {
        List<SpecieEntity> entityList = new ArrayList<>();
        if(DTOList != null) {
            entityList.addAll(DTOList.stream().map(this::toEntity).toList());
        }
        return entityList;
    }

    default List<Specie> toDomainEntityFromEntityLists(List<SpecieEntity> entityList) {
        List<Specie> domainEntityList = new ArrayList<>();
        if(entityList != null) {
            domainEntityList.addAll(entityList.stream().map(this::toDomainEntity).toList());
        }
        return domainEntityList;
    }
}